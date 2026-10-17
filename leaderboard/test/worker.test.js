import assert from 'node:assert/strict';
import { test } from 'node:test';

import vectors from './vectors.json' with { type: 'json' };
import { TOP, handle, insert, isoWeek, listKey } from '../src/worker.js';

const ORIGIN = 'https://timefighter.trendgegner.com';
const NOW = new Date('2026-10-17T12:00:00Z');

/** KV namespace in memory, recording TTLs. */
function memoryKv() {
  const data = new Map();
  const ttl = new Map();
  return {
    data,
    ttl,
    async get(key) {
      return data.get(key) ?? null;
    },
    async put(key, value, options = {}) {
      data.set(key, value);
      if (options.expirationTtl) ttl.set(key, options.expirationTtl);
    },
  };
}

function post(body, { ip = '192.0.2.1', origin = ORIGIN } = {}) {
  return new Request('https://api.example/scores', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'CF-Connecting-IP': ip, Origin: origin },
    body: JSON.stringify(body),
  });
}

function entry(vector, name = 'MAVERICK') {
  return {
    name,
    mode: vector.mode,
    level: vector.level,
    score: vector.score,
    duration_s: vector.duration_s,
    version: '0.1.0',
    breakdown: vector.breakdown,
  };
}

test('a score goes on the all-time and the weekly list and can be read back', async () => {
  const env = { SCORES: memoryKv() };
  const vector = vectors[0];
  const response = await handle(post(entry(vector)), env, NOW);
  assert.equal(response.status, 200);
  assert.equal(response.headers.get('Access-Control-Allow-Origin'), ORIGIN);
  const answer = await response.json();
  assert.equal(answer.rank_all, 1);
  assert.equal(answer.rank_week, 1);
  // Nothing but name, score and date is kept.
  assert.deepEqual(answer.top, [{ name: 'MAVERICK', score: vector.score, date: '2026-10-17' }]);

  const weekKey = listKey('week', vector.mode, vector.level, NOW);
  assert.equal(weekKey, `week:2026-W42:${vector.mode}:${vector.level}`);
  assert.ok(env.SCORES.ttl.get(weekKey) > 7 * 24 * 3600);
  assert.equal(env.SCORES.ttl.get(`all:${vector.mode}:${vector.level}`), undefined);

  const read = await handle(
    new Request(`https://api.example/scores?mode=${vector.mode}&level=${vector.level}&period=week`),
    env,
    NOW,
  );
  const { entries } = await read.json();
  assert.equal(entries.length, 1);
  assert.equal(entries[0].name, 'MAVERICK');
});

test('one score per minute per address, and no address is stored', async () => {
  const env = { SCORES: memoryKv() };
  const vector = vectors[0];
  assert.equal((await handle(post(entry(vector)), env, NOW)).status, 200);
  assert.equal((await handle(post(entry(vector, 'GOOSE')), env, NOW)).status, 429);
  assert.equal((await handle(post(entry(vector, 'GOOSE'), { ip: '192.0.2.2' }), env, NOW)).status, 200);
  const stored = [...env.SCORES.data.entries()].map(([k, v]) => k + v).join('\n');
  assert.ok(!stored.includes('192.0.2'), 'addresses only as hashes');
  const rateKeys = [...env.SCORES.ttl.entries()].filter(([k]) => k.startsWith('rl:'));
  assert.equal(rateKeys.length, 2);
  assert.ok(rateKeys.every(([, ttl]) => ttl === 60));
});

test('foreign origins, bad scores and unknown paths are refused', async () => {
  const env = { SCORES: memoryKv() };
  const vector = vectors[0];
  assert.equal((await handle(post(entry(vector), { origin: 'https://evil.example' }), env, NOW)).status, 403);
  const bad = await handle(post({ ...entry(vector), score: 1 }), env, NOW);
  assert.equal(bad.status, 422);
  assert.equal((await bad.json()).error, 'score does not add up');
  assert.equal((await handle(new Request('https://api.example/other'), env, NOW)).status, 404);
  const preflight = await handle(new Request('https://api.example/scores', { method: 'OPTIONS' }), env, NOW);
  assert.equal(preflight.status, 204);
  assert.match(preflight.headers.get('Access-Control-Allow-Methods'), /POST/);
  const custom = await handle(new Request('https://api.example/scores?mode=hunt&level=custom'), env, NOW);
  assert.equal(custom.status, 400);
  assert.equal(env.SCORES.data.size, 0, 'nothing stored');
});

test('lists keep the best hundred, ties in order of arrival', () => {
  const list = [];
  for (let i = 0; i < TOP; i++) insert(list, { name: `P${i}`, score: 1000 - i });
  assert.equal(insert(list, { name: 'LOW', score: 1 }), null);
  assert.equal(insert(list, { name: 'TIE', score: 1000 }), 2);
  assert.equal(list.length, TOP);
  assert.equal(list[0].name, 'P0');
});

test('iso weeks', () => {
  assert.equal(isoWeek(new Date('2026-01-01T00:00:00Z')), '2026-W01');
  assert.equal(isoWeek(new Date('2027-01-01T00:00:00Z')), '2026-W53');
  assert.equal(isoWeek(NOW), '2026-W42');
});
