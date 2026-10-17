import assert from 'node:assert/strict';
import { test } from 'node:test';

import limits from '../limits.json' with { type: 'json' };
import vectors from './vectors.json' with { type: 'json' };
import { checkSubmission, score } from '../src/score.js';
import { checkName, cleanName } from '../src/names.js';

function submission(vector, changes = {}) {
  return {
    name: 'MAVERICK',
    mode: vector.mode,
    level: vector.level,
    score: vector.score,
    duration_s: vector.duration_s,
    version: '0.1.0',
    breakdown: structuredClone(vector.breakdown),
    ...changes,
  };
}

test('every example round scores as in the game', () => {
  assert.ok(vectors.length >= 6);
  for (const vector of vectors) {
    const level = limits[vector.mode][vector.level];
    assert.equal(score(vector.breakdown, vector.duration_s, level), vector.score, JSON.stringify(vector));
    const checked = checkSubmission(submission(vector), limits);
    assert.ok(checked.ok, `${vector.mode} ${vector.level}: ${checked.reason}`);
    assert.equal(checked.entry.score, vector.score);
  }
});

test('a score that does not add up is refused', () => {
  const vector = vectors[0];
  const checked = checkSubmission(submission(vector, { score: vector.score + 1 }), limits);
  assert.deepEqual(checked, { ok: false, reason: 'score does not add up' });
});

test('kills beyond the mission, or a victory without all of them, are refused', () => {
  const won = vectors.find((v) => v.breakdown.victory && v.mode === 'hunt');
  const more = submission(won);
  more.breakdown.kills.bf109k4.gun += 50;
  assert.match(checkSubmission(more, limits).reason, /more fighters/);

  const fewer = submission(won);
  fewer.breakdown.kills.bf109k4.gun -= 1;
  assert.equal(checkSubmission(fewer, limits).reason, 'victory without every kill');

  const zeppelin = submission(won);
  zeppelin.breakdown.zeppelins_outside = 1;
  assert.equal(checkSubmission(zeppelin, limits).reason, 'bad zeppelin bonus');

  const missiles = submission(won);
  missiles.breakdown.missiles_left = 99;
  assert.equal(checkSubmission(missiles, limits).reason, 'bad missiles');

  const city = submission(won);
  city.breakdown.city_percent = 50;
  assert.equal(checkSubmission(city, limits).reason, 'no city in this mode');

  const alien = submission(won);
  alien.breakdown.kills.ufo = { gun: 1 };
  assert.equal(checkSubmission(alien, limits).reason, 'unknown enemy ufo');
});

test('fighters launched by zeppelins count up to the hangar limit', () => {
  const vector = vectors.find((v) => v.mode === 'battle' && v.level === 'normal');
  const level = limits.battle.normal;
  assert.ok(level.fighters_max > level.fighters);
  const extra = submission(vector);
  const launched = level.fighters_max - level.fighters;
  extra.breakdown.kills.me309.missile = (extra.breakdown.kills.me309.missile ?? 0) + launched;
  extra.duration_s += 3 * launched;
  extra.score = score(extra.breakdown, extra.duration_s, level);
  assert.ok(checkSubmission(extra, limits).ok, checkSubmission(extra, limits).reason);
  extra.breakdown.kills.me309.missile += 1;
  extra.score = score(extra.breakdown, extra.duration_s, level);
  assert.match(checkSubmission(extra, limits).reason, /more fighters/);
});

test('rounds faster than plausible are refused', () => {
  const vector = vectors[0];
  const kills = Object.values(vector.breakdown.kills)
    .flatMap((w) => Object.values(w))
    .reduce((a, b) => a + b, 0);
  const fast = submission(vector, { duration_s: 15 + 3 * kills - 1 });
  // Faster also changes the time bonus, so fix the score first.
  fast.score = score(fast.breakdown, fast.duration_s, limits[vector.mode][vector.level]);
  assert.equal(checkSubmission(fast, limits).reason, 'too fast');
});

test('custom levels and unknown modes do not go online', () => {
  const vector = vectors[0];
  assert.equal(checkSubmission(submission(vector, { level: 'custom' }), limits).reason, 'unknown mode or level');
  assert.equal(checkSubmission(submission(vector, { mode: 'dogfight' }), limits).reason, 'unknown mode or level');
});

test('names take any printable characters, cleaned, up to eighteen', () => {
  for (const good of ['Trendgegner', 'A', 'ICE  MAN', 'Jürgen Weiß', 'Пилот', '飛行士', '<b>&"x"', 'eighteen-chars-xyz', 'PILOT 1988']) {
    assert.equal(checkName(cleanName(good)), null, good);
  }
  assert.equal(cleanName('  Mav\u0007er\u200Bick \n'), 'Maverick');
  assert.equal(cleanName('Cafe\u0301'), 'Café', 'NFC');
  for (const bad of ['', '   ', '\u200B', 'nineteen-characters']) {
    assert.notEqual(checkName(cleanName(bad)), null, JSON.stringify(bad));
  }
  for (const blocked of ['HITLER', 'Hítler', 'N4ZI', 'Sieg Heil', 'ACE 88', 'X1488', 'F_U_C_K']) {
    assert.equal(checkName(cleanName(blocked)), 'name not allowed', blocked);
  }
});

test('the stored name is the cleaned one', () => {
  const vector = vectors[0];
  const checked = checkSubmission(submission(vector, { name: '  Jürgen\u200B ✈\t' }), limits);
  assert.ok(checked.ok, checked.reason);
  assert.equal(checked.entry.name, 'Jürgen ✈');
  assert.equal(checkSubmission(submission(vector, { name: ' \u0000 ' }), limits).reason, 'no name');
});
