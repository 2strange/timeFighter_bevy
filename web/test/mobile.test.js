// The overlay's tilt maths without a browser: run with `node --test web/test/*.test.js`.

import assert from 'node:assert/strict';
import { test } from 'node:test';

globalThis.matchMedia = () => ({ matches: false });
globalThis.document = { documentElement: { dataset: {} } };
const { tiltAngles } = await import('../mobile.js');

const near = (a, b, what) => assert.ok(Math.abs(a - b) < 0.01, `${what}: ${a} vs ${b}`);

test('upright is level in portrait and both landscape ways', () => {
  for (const [beta, gamma, angle] of [
    [90, 0, 0],
    [0, -90, 90],
    [0, 90, 270],
  ]) {
    const { pitch, roll } = tiltAngles(beta, gamma, angle);
    near(pitch, 0, `pitch ${angle}`);
    near(roll, 0, `roll ${angle}`);
  }
});

test('tipping the top edge away pitches positive', () => {
  near(tiltAngles(60, 0, 0).pitch, 30, 'portrait');
  near(tiltAngles(0, -70, 90).pitch, 20, 'landscape');
  near(tiltAngles(0, 70, 270).pitch, 20, 'landscape the other way');
});

test('right side down rolls positive', () => {
  assert.ok(tiltAngles(80, 20, 0).roll > 0, 'portrait');
  // Landscape (screen angle 90, the phone turned counter-clockwise): its top
  // is on the left, so the right side going down raises the top: beta > 0.
  assert.ok(tiltAngles(20, -70, 90).roll > 0, 'landscape');
  assert.ok(tiltAngles(-20, -70, 90).roll < 0, 'landscape, left down');
  // Turned the other way (270) it is mirrored.
  assert.ok(tiltAngles(-20, 70, 270).roll > 0, 'landscape the other way');
});
