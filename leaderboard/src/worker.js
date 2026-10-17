// TimeFighter hall of fame: a Cloudflare Worker with one KV namespace
// (binding SCORES).
//
//   POST /scores  { name, mode, level, score, duration_s, version, breakdown }
//                 -> { rank_all, rank_week, top }   (ranks 1-based or null)
//   GET  /scores?mode=hunt&level=normal&period=all|week[&limit=10]
//                 -> { entries: [{ name, score, date }] }
//
// KV keys: "all:<mode>:<level>" and "week:<yyyy>-W<ww>:<mode>:<level>"
// hold the best 100 as JSON; "rl:<hash>" marks a recent submission for the
// rate limit. Stored per score: name, score and date (mode and level are in
// the key). No IP address is stored: the rate limit keeps only a salted
// hash, for a minute.

import limits from '../limits.json' with { type: 'json' };
import { checkSubmission } from './score.js';

export const TOP = 100;
const WEEK_TTL = 21 * 24 * 3600;
/** Seconds between two submissions from one address (KV's shortest TTL). */
const RATE_TTL = 60;
const MAX_BODY = 4096;

export default {
  async fetch(request, env) {
    return handle(request, env, new Date());
  },
};

export async function handle(request, env, now) {
  const origin = env.ALLOWED_ORIGIN || 'https://timefighter.trendgegner.com';
  const cors = {
    'Access-Control-Allow-Origin': origin,
    'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type',
    'Access-Control-Max-Age': '86400',
    Vary: 'Origin',
  };
  const reply = (status, body, extra = {}) =>
    new Response(body === null ? null : JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json', ...cors, ...extra },
    });

  const url = new URL(request.url);
  if (url.pathname !== '/scores') return reply(404, { error: 'not found' });
  if (request.method === 'OPTIONS') return reply(204, null);
  const from = request.headers.get('Origin');
  if (from && from !== origin) return reply(403, { error: 'origin not allowed' });

  if (request.method === 'GET') {
    const mode = url.searchParams.get('mode');
    const level = url.searchParams.get('level');
    if (!limits[mode]?.[level]) return reply(400, { error: 'unknown mode or level' });
    const period = url.searchParams.get('period') === 'week' ? 'week' : 'all';
    const limit = Math.min(Math.max(Number(url.searchParams.get('limit')) || 10, 1), TOP);
    const list = await readList(env, listKey(period, mode, level, now));
    return reply(200, { entries: list.slice(0, limit) }, { 'Cache-Control': 'max-age=30' });
  }

  if (request.method !== 'POST') return reply(405, { error: 'method not allowed' });
  const text = await request.text();
  if (text.length > MAX_BODY) return reply(413, { error: 'too large' });
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return reply(400, { error: 'not JSON' });
  }
  const checked = checkSubmission(body, limits);
  if (!checked.ok) return reply(422, { error: checked.reason });

  const address = request.headers.get('CF-Connecting-IP') || 'unknown';
  const rateKey = `rl:${await hash(`${env.RATE_SALT || ''}:${day(now)}:${address}`)}`;
  if (await env.SCORES.get(rateKey)) return reply(429, { error: 'one score per minute' });
  await env.SCORES.put(rateKey, '1', { expirationTtl: RATE_TTL });

  const entry = { ...checked.entry, date: day(now) };
  const all = listKey('all', body.mode, body.level, now);
  const week = listKey('week', body.mode, body.level, now);
  const allList = await readList(env, all);
  const rankAll = insert(allList, entry);
  await env.SCORES.put(all, JSON.stringify(allList));
  const weekList = await readList(env, week);
  const rankWeek = insert(weekList, entry);
  await env.SCORES.put(week, JSON.stringify(weekList), { expirationTtl: WEEK_TTL });
  return reply(200, { rank_all: rankAll, rank_week: rankWeek, top: allList.slice(0, 10) });
}

export function listKey(period, mode, level, now) {
  return period === 'week' ? `week:${isoWeek(now)}:${mode}:${level}` : `all:${mode}:${level}`;
}

async function readList(env, key) {
  const text = await env.SCORES.get(key);
  if (!text) return [];
  try {
    const list = JSON.parse(text);
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}

/** Inserts below equal scores, keeps the best TOP; returns the rank or null. */
export function insert(list, entry) {
  let place = list.findIndex((e) => e.score < entry.score);
  if (place < 0) place = list.length;
  if (place >= TOP) return null;
  list.splice(place, 0, entry);
  list.length = Math.min(list.length, TOP);
  return place + 1;
}

export function day(now) {
  return now.toISOString().slice(0, 10);
}

/** ISO 8601 week, e.g. "2026-W42". */
export function isoWeek(now) {
  const date = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()));
  const weekday = date.getUTCDay() || 7;
  date.setUTCDate(date.getUTCDate() + 4 - weekday);
  const yearStart = new Date(Date.UTC(date.getUTCFullYear(), 0, 1));
  const week = Math.ceil(((date - yearStart) / 86400000 + 1) / 7);
  return `${date.getUTCFullYear()}-W${String(week).padStart(2, '0')}`;
}

async function hash(text) {
  const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, '0')).join('');
}
