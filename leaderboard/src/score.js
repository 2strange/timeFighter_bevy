// The game's scoring rules (src/score/rules.rs), to recompute every
// submitted score from its breakdown, and the checks that keep a score
// within what the mission allows. test/vectors.json (written by the game's
// tests) holds example rounds both sides must score alike.

import { checkName, cleanName } from './names.js';

export const KILL_POINTS = { bf109k4: 100, me309: 150, he177: 200, ju390: 400, zeppelin: 1500 };
const CLASS = {
  bf109k4: 'fighters',
  me309: 'fighters',
  he177: 'bombers',
  ju390: 'bombers',
  zeppelin: 'zeppelins',
};
const WEAPONS = ['gun', 'missile', 'crash'];
export const ZEPPELIN_OUTSIDE = 500;
export const PER_MISSILE_LEFT = 75;
export const PER_CITY_PERCENT = 20;
export const VICTORY = 1000;
export const PER_SECOND_UNDER_PAR = 5;
/** Shortest plausible round: this much plus a few seconds per kill. */
export const MIN_SECONDS = 15;
export const MIN_SECONDS_PER_KILL = 3;
/** Longest round accepted (s). */
export const MAX_SECONDS = 3 * 3600;

function count(value, max) {
  return Number.isInteger(value) && value >= 0 && value <= max;
}

/**
 * The total of a breakdown at a level (`{ factor, par_s }` from
 * limits.json). Assumes a breakdown that passed `checkBreakdown`.
 */
export function score(breakdown, durationS, level) {
  let kills = 0;
  for (const [kind, byWeapon] of Object.entries(breakdown.kills)) {
    const base = KILL_POINTS[kind];
    kills += (byWeapon.gun ?? 0) * ((base * 3) / 2);
    kills += ((byWeapon.missile ?? 0) + (byWeapon.crash ?? 0)) * base;
  }
  let subtotal = kills + breakdown.zeppelins_outside * ZEPPELIN_OUTSIDE;
  if (breakdown.victory) {
    subtotal += breakdown.missiles_left * PER_MISSILE_LEFT;
    subtotal += Math.min(breakdown.city_percent ?? 0, 100) * PER_CITY_PERCENT;
    subtotal += VICTORY;
    subtotal += Math.max(0, level.par_s - durationS) * PER_SECOND_UNDER_PAR;
  }
  return Math.round(subtotal * level.factor);
}

/** Kills per class: `{ fighters, bombers, zeppelins, total }`. */
export function classCounts(kills) {
  const counts = { fighters: 0, bombers: 0, zeppelins: 0, total: 0 };
  for (const [kind, byWeapon] of Object.entries(kills)) {
    for (const n of Object.values(byWeapon)) {
      counts[CLASS[kind]] += n;
      counts.total += n;
    }
  }
  return counts;
}

/** A reason why the breakdown cannot come from a round at `level`, or null. */
export function checkBreakdown(breakdown, level) {
  if (typeof breakdown !== 'object' || breakdown === null) return 'no breakdown';
  const { kills, zeppelins_outside, missiles_left, city_percent, victory } = breakdown;
  if (typeof kills !== 'object' || kills === null || Array.isArray(kills)) return 'bad kills';
  for (const [kind, byWeapon] of Object.entries(kills)) {
    if (!(kind in KILL_POINTS)) return `unknown enemy ${kind}`;
    if (typeof byWeapon !== 'object' || byWeapon === null) return 'bad kills';
    for (const [weapon, n] of Object.entries(byWeapon)) {
      if (!WEAPONS.includes(weapon) || !count(n, 1000)) return 'bad kills';
    }
  }
  if (typeof victory !== 'boolean') return 'bad victory';
  const counts = classCounts(kills);
  // Zeppelins may launch fighters on top of the waves (fighters_max).
  const most = { fighters: level.fighters_max, bombers: level.bombers, zeppelins: level.zeppelins };
  for (const cls of ['fighters', 'bombers', 'zeppelins']) {
    if (counts[cls] > most[cls]) return `more ${cls} than the mission has`;
    // A victory means everything is down, the waves at least.
    if (victory && counts[cls] < level[cls]) return 'victory without every kill';
  }
  if (!count(zeppelins_outside, counts.zeppelins)) return 'bad zeppelin bonus';
  if (!count(missiles_left, level.missiles)) return 'bad missiles';
  if (level.city) {
    if (!(city_percent === null || count(city_percent, 100))) return 'bad city';
  } else if (city_percent !== null) {
    return 'no city in this mode';
  }
  return null;
}

/**
 * Checks a submission against limits.json. Returns `{ ok: true, entry }`
 * with what is stored, or `{ ok: false, reason }`.
 */
export function checkSubmission(body, limits) {
  if (typeof body !== 'object' || body === null) return { ok: false, reason: 'not an object' };
  const { mode, level, score: claimed, duration_s, version, breakdown } = body;
  const name = cleanName(body.name);
  const nameProblem = checkName(name);
  if (nameProblem) return { ok: false, reason: nameProblem };
  const limit = limits[mode]?.[level];
  if (!limit) return { ok: false, reason: 'unknown mode or level' };
  if (typeof version !== 'string' || version.length > 20) return { ok: false, reason: 'bad version' };
  if (!count(duration_s, MAX_SECONDS)) return { ok: false, reason: 'bad duration' };
  const problem = checkBreakdown(breakdown, limit);
  if (problem) return { ok: false, reason: problem };
  const kills = classCounts(breakdown.kills).total;
  if (duration_s < MIN_SECONDS + MIN_SECONDS_PER_KILL * kills) {
    return { ok: false, reason: 'too fast' };
  }
  const total = score(breakdown, duration_s, limit);
  if (claimed !== total) return { ok: false, reason: 'score does not add up' };
  return { ok: true, entry: { name, score: total } };
}
