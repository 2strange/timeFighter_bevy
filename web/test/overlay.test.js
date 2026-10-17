// The touch overlay over several rounds, in a small stand-in for the DOM:
// nothing piles up per round (listeners, nodes), and QUIT TO START keeps
// working. Run with `node --test web/test/*.test.js`.

import assert from 'node:assert/strict';
import { test } from 'node:test';

const counts = { window: 0, document: 0, nodes: 0, observers: 0 };
let observer = null;

class Node {
  constructor(tag) {
    this.tag = tag;
    this.children = [];
    this.listeners = {};
    this.dataset = {};
    this.style = {};
    this.attributes = {};
    this.className = '';
    this.textContent = '';
    this.value = '';
    const classes = new Set();
    this.classList = {
      add: (c) => classes.add(c),
      toggle: (c, on) => (on ?? !classes.has(c) ? classes.add(c) : classes.delete(c)),
      contains: (c) => classes.has(c),
    };
    counts.nodes += 1;
  }
  append(...nodes) {
    this.children.push(...nodes);
  }
  addEventListener(type, fn) {
    (this.listeners[type] ??= []).push(fn);
  }
  setAttribute(k, v) {
    this.attributes[k] = v;
  }
  setPointerCapture() {}
  hasPointerCapture() {
    return false;
  }
  blur() {}
  getBoundingClientRect() {
    return { left: 0, top: 0, width: 100, height: 100 };
  }
  fire(type) {
    for (const fn of this.listeners[type] ?? []) fn({ preventDefault() {}, pointerId: 1, clientX: 0, clientY: 0 });
  }
  find(text) {
    if (this.textContent === text) return this;
    for (const child of this.children) {
      const hit = child.find?.(text);
      if (hit) return hit;
    }
    return null;
  }
}

const root = new Node('html');
const replaced = [];
globalThis.matchMedia = () => ({ matches: true });
globalThis.document = {
  documentElement: root,
  head: new Node('head'),
  body: new Node('body'),
  createElement: (tag) => new Node(tag),
  addEventListener: () => (counts.document += 1),
  visibilityState: 'visible',
};
// With a gyro that needs no permission (Android): its listener must not
// be added again on a second start.
globalThis.window = {
  addEventListener: () => (counts.window += 1),
  DeviceOrientationEvent: class {},
};
globalThis.setTimeout = () => 0;
globalThis.MutationObserver = class {
  constructor(fn) {
    observer = fn;
    counts.observers += 1;
  }
  observe() {}
};
globalThis.localStorage = { removeItem() {}, getItem: () => null, setItem() {} };
globalThis.location = { pathname: '/', replace: (url) => replaced.push(url) };
Object.defineProperty(globalThis, 'navigator', { value: { userAgent: 'test' }, configurable: true });
Object.defineProperty(globalThis, 'screen', { value: {}, configurable: true });

const { startTouch } = await import('../mobile.js');

function mode(m) {
  root.dataset.tfMode = m;
  observer();
}

test('rounds do not pile up listeners or nodes, and quit keeps working', () => {
  startTouch();
  const after = { ...counts };
  const overlay = document.body.children[0];
  for (let round = 0; round < 5; round += 1) {
    // A start tap again (it happens once per page, but must stay harmless).
    startTouch();
    for (const m of ['play', 'paused', 'play', 'dialog', 'over', 'menu', 'play']) mode(m);
    overlay.find('FLY AGAIN').fire('pointerdown');
  }
  assert.deepEqual(counts, after, 'no new listeners, observers or nodes');
  assert.equal(document.body.children.length, 1, 'one overlay');

  // Both QUIT TO START buttons (pause screen, portrait hint) still go.
  const quits = [];
  const walk = (node) => {
    if (node.textContent === 'QUIT TO START') quits.push(node);
    node.children.forEach(walk);
  };
  walk(overlay);
  assert.equal(quits.length, 2);
  for (const quit of quits) quit.fire('pointerdown');
  assert.equal(replaced.length, 2);
  assert.ok(replaced.every((url) => url.startsWith('/?quit=')));
  // The fly-again taps were counted, one per round.
  assert.match(root.dataset.tfInput, /;a=5;/);
});
