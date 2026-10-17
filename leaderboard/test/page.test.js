// The page's side (web/leaderboard.js) against the worker, without a
// browser: the game's outbox goes out and the answer comes back in the
// form the game reads (src/score/store.rs).

import assert from 'node:assert/strict';
import { test } from 'node:test';

import vectors from './vectors.json' with { type: 'json' };
import { handle } from '../src/worker.js';

const storage = new Map();
const kv = new Map();
let tick = null;

globalThis.window = {
  localStorage: {
    getItem: (k) => storage.get(k) ?? null,
    setItem: (k, v) => storage.set(k, String(v)),
  },
};
globalThis.document = {
  querySelector: (selector) =>
    selector === 'meta[name="tf-leaderboard"]' ? { content: 'https://api.example/' } : null,
};
globalThis.setInterval = (fn) => {
  tick = fn;
  return 1;
};
globalThis.fetch = (url, init = {}) =>
  handle(
    new Request(url, {
      ...init,
      headers: { ...init.headers, Origin: 'https://timefighter.trendgegner.com', 'CF-Connecting-IP': '198.51.100.7' },
    }),
    {
      SCORES: {
        get: async (k) => kv.get(k) ?? null,
        put: async (k, v) => kv.set(k, v),
      },
    },
    new Date('2026-10-17T12:00:00Z'),
  );

const page = await import('../../web/leaderboard.js');

async function settle() {
  for (let i = 0; i < 20; i++) await new Promise((resolve) => setImmediate(resolve));
}

test('the address comes from the meta tag, without a trailing slash', () => {
  assert.equal(page.api, 'https://api.example');
});

test('a queued score is sent once and the answer is left for the game', async () => {
  const vector = vectors[0];
  const item = {
    id: 3,
    name: 'MAVERICK',
    mode: vector.mode,
    level: vector.level,
    score: vector.score,
    duration_s: vector.duration_s,
    version: '0.1.0',
    breakdown: vector.breakdown,
  };
  storage.set('timefighter.outbox', JSON.stringify([item]));
  page.pumpOutbox();
  tick();
  assert.equal(storage.get('timefighter.outbox'), '[]', 'emptied before sending');
  await settle();
  assert.equal(storage.get('timefighter.online'), `3\n1\n1\nMAVERICK|${vector.score}`);

  // A refused one comes back as an error line.
  storage.set('timefighter.outbox', JSON.stringify([{ ...item, id: 4, score: 1 }]));
  tick();
  await settle();
  assert.equal(storage.get('timefighter.online'), '4\nerror\nscore does not add up');
});

test('local lists are read in the game\'s form', () => {
  storage.set('timefighter.top.hunt.normal', 'ACE|900;bad;BOB|500');
  assert.deepEqual(page.localList('hunt', 'normal'), [
    { name: 'ACE', score: 900 },
    { name: 'BOB', score: 500 },
  ]);
});
