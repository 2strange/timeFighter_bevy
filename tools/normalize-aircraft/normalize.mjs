#!/usr/bin/env node
// Normalises aircraft glTF models once, so the game can load them without any
// correction factor: 1 unit = 1 m, +Y up, nose along -Z, origin at the centre
// of the bounding box in X/Z and on the fuselage axis in Y. All node
// transforms are baked into the vertices, animations are dropped (an optional
// pose from an animation keyframe is baked first, e.g. landing gear up), and
// textures are embedded into a single .glb.
//
// Usage:
//   node normalize.mjs --source-root <dir> [--manifest ../aircraft.json]
//                      [--out ../../assets/aircraft] [--only <name>]
//
// Every manifest entry names a source folder (scene.gltf + license.txt),
// the real length in metres, and the source's forward/up axes ("+X", "-Z",
// ...). "auto" lets the script guess and print its reasoning; pin the guess
// in the manifest once it has been checked visually.

import { readFile, writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { NodeIO, getBounds } from '@gltf-transform/core';
import { clearNodeTransform, dedup, flatten, join, prune, transformMesh } from '@gltf-transform/functions';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = parseArgs(process.argv.slice(2));
const manifestPath = path.resolve(args.manifest ?? path.join(here, '..', 'aircraft.json'));
const outDir = path.resolve(args.out ?? path.join(here, '..', '..', 'assets', 'aircraft'));
if (!args['source-root']) fail('missing --source-root <dir>');
const sourceRoot = path.resolve(args['source-root']);

// Only licences that allow use in a public, possibly commercial project.
const ALLOWED_LICENSES = ['CC-BY-4.0', 'CC0-1.0', 'CC0'];

const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
const io = new NodeIO();
await mkdir(outDir, { recursive: true });

const credits = [];
for (const entry of manifest.aircraft) {
  if (args.only && args.only !== entry.name) continue;
  const source = path.join(sourceRoot, entry.source);
  const license = parseLicense(await readFile(path.join(source, 'license.txt'), 'utf8'));
  if (!ALLOWED_LICENSES.includes(license.license)) {
    fail(`${entry.name}: licence "${license.license}" is not allowed (${ALLOWED_LICENSES.join(', ')})`);
  }
  credits.push({ ...license, file: `${entry.name}.glb` });

  const doc = await io.read(path.join(source, 'scene.gltf'));
  const report = await normalize(doc, entry);
  const out = path.join(outDir, `${entry.name}.glb`);
  await io.write(out, doc);
  console.log(formatReport(entry, report, out));
}

if (!args.only) {
  await writeFile(path.join(outDir, 'CREDITS.md'), formatCredits(credits));
  console.log(`credits: ${path.join(outDir, 'CREDITS.md')}`);
}

async function normalize(doc, entry) {
  const root = doc.getRoot();
  const scene = root.getDefaultScene() ?? root.listScenes()[0];

  const pose = choosePose(doc, scene, entry);
  for (const animation of root.listAnimations()) animation.dispose();

  // Bake every node transform into its mesh, then hang all meshes below one
  // clean root node.
  await doc.transform(flatten());
  const meshNodes = [];
  scene.traverse((node) => {
    if (node.getMesh()) meshNodes.push(node);
  });
  for (const node of meshNodes) clearNodeTransform(node);
  const modelRoot = doc.createNode(entry.name);
  for (const child of scene.listChildren()) scene.removeChild(child);
  for (const node of meshNodes) {
    node.listChildren().forEach((c) => node.removeChild(c));
    modelRoot.addChild(node);
  }
  scene.addChild(modelRoot);
  await doc.transform(prune(), dedup());

  const positions = collectPositions(meshNodes);
  const before = boundsOf(positions);
  const axes = chooseAxes(positions, before, entry);

  // Rotation rows: source right, up, back (= -forward) become target X, Y, Z.
  const f = axisVector(axes.forward);
  const u = axisVector(axes.up);
  const r = cross(f, u);
  const b = f.map((v) => -v);
  const lengthBefore = extentAlong(before, f);
  const scale = entry.length / lengthBefore;

  // Origin: box centre in X/Z, fuselage axis (mean height of the nose region) in Y.
  const rotated = positions.map((p) => [dot(r, p) * scale, dot(u, p) * scale, dot(b, p) * scale]);
  const box = boundsOf(rotated);
  const noseCut = box.min[2] + 0.05 * (box.max[2] - box.min[2]);
  const nose = rotated.filter((p) => p[2] <= noseCut);
  const centre = [
    (box.min[0] + box.max[0]) / 2,
    nose.reduce((s, p) => s + p[1], 0) / nose.length,
    (box.min[2] + box.max[2]) / 2,
  ];

  // Column-major 4x4: translate(-centre) * scale * rotation.
  const m = [
    r[0] * scale, u[0] * scale, b[0] * scale, 0,
    r[1] * scale, u[1] * scale, b[1] * scale, 0,
    r[2] * scale, u[2] * scale, b[2] * scale, 0,
    -centre[0], -centre[1], -centre[2], 1,
  ];
  for (const mesh of root.listMeshes()) transformMesh(mesh, m);

  await doc.transform(join({ keepNamed: false }), prune(), dedup());
  const after = getBounds(scene);
  const counts = {
    meshes: root.listMeshes().length,
    primitives: root.listMeshes().reduce((s, mesh) => s + mesh.listPrimitives().length, 0),
    textures: root.listTextures().length,
  };
  return { pose, axes, before, after, scale, counts };
}

// --- Pose -------------------------------------------------------------------

/// Applies the requested pose from the first animation: "rest" (node
/// transforms as stored), "first"/"last" keyframe, or "auto" = the candidate
/// with the smallest extent along the up axis (landing gear retracted).
function choosePose(doc, scene, entry) {
  const animation = doc.getRoot().listAnimations()[0];
  if (!animation) return { chosen: 'rest', note: 'no animation' };

  const rest = new Map();
  for (const channel of animation.listChannels()) {
    const node = channel.getTargetNode();
    if (node && !rest.has(node)) {
      rest.set(node, {
        t: node.getTranslation(),
        r: node.getRotation(),
        s: node.getScale(),
      });
    }
  }
  const restore = () => {
    for (const [node, trs] of rest) node.setTranslation(trs.t).setRotation(trs.r).setScale(trs.s);
  };
  const applyKey = (which) => {
    restore();
    for (const channel of animation.listChannels()) {
      const node = channel.getTargetNode();
      const sampler = channel.getSampler();
      const output = sampler.getOutput();
      const count = sampler.getInput().getCount();
      const index = which === 'first' ? 0 : count - 1;
      const value = output.getElement(index, []);
      const pathName = channel.getTargetPath();
      if (pathName === 'translation') node.setTranslation(value);
      else if (pathName === 'rotation') node.setRotation(value);
      else if (pathName === 'scale') node.setScale(value);
    }
  };

  // Gear check: how far the lowest animated part sits above the model's
  // lowest point, as a fraction of the height. Retracted gear scores high.
  const animated = [...rest.keys()];
  const upAxis = entry.up && entry.up !== 'auto' ? entry.up : null;
  const candidates = {};
  for (const candidate of ['rest', 'first', 'last']) {
    if (candidate === 'rest') restore();
    else applyKey(candidate);
    const bounds = getBounds(scene);
    const extents = bounds.max.map((v, i) => v - bounds.min[i]);
    const k = upAxis ? axisIndex(upAxis) : extents.indexOf(Math.min(...extents));
    const sign = upAxis && upAxis[0] === '-' ? -1 : 1;
    const low = (b) => (sign > 0 ? b.min[k] : -b.max[k]);
    const partsLow = Math.min(...animated.map((node) => low(getBounds(node))).filter(Number.isFinite));
    candidates[candidate] = { extents, tuck: (partsLow - low(bounds)) / extents[k], height: extents[k] };
  }
  let chosen = entry.pose ?? 'auto';
  if (chosen === 'auto') {
    chosen = Object.keys(candidates).reduce((best, c) => {
      const [x, y] = [candidates[c], candidates[best]];
      if (x.tuck > y.tuck + 0.01) return c;
      if (Math.abs(x.tuck - y.tuck) <= 0.01 && x.height < y.height - 1e-6) return c;
      return best;
    }, 'rest');
  }
  if (chosen === 'rest') restore();
  else applyKey(chosen);
  return { chosen, candidates, animation: animation.getName() };
}

// --- Axes -------------------------------------------------------------------

function chooseAxes(positions, bounds, entry) {
  const extents = bounds.max.map((v, i) => v - bounds.min[i]);
  const notes = [];

  let up = entry.up;
  if (!up || up === 'auto') {
    const k = extents.indexOf(Math.min(...extents));
    const values = positions.map((p) => p[k]).sort((a, c) => a - c);
    const median = values[Math.floor(values.length / 2)];
    const above = bounds.max[k] - median;
    const below = median - bounds.min[k];
    up = `${above >= below ? '+' : '-'}${'XYZ'[k]}`;
    notes.push(`up ${up}: smallest extent; above/below median ${above.toFixed(2)}/${below.toFixed(2)}`);
  }

  let forward = entry.forward;
  if (!forward || forward === 'auto') {
    const u = axisIndex(up);
    const horizontal = [0, 1, 2].filter((k) => k !== u);
    // The span axis is mirror-symmetric, the length axis is not.
    const asym = horizontal.map((k) => asymmetry(positions, bounds, k));
    const k = horizontal[asym[0] > asym[1] ? 0 : 1];
    // The fin sits at the tail: vertices in the top 15 % of the height.
    const upSign = up[0] === '-' ? -1 : 1;
    const heights = positions.map((p) => p[u] * upSign);
    const top = Math.max(...heights);
    const bottom = Math.min(...heights);
    const cut = top - 0.15 * (top - bottom);
    const centre = (bounds.min[k] + bounds.max[k]) / 2;
    const fin = positions.filter((p, i) => heights[i] >= cut);
    const finOffset = fin.reduce((s, p) => s + (p[k] - centre), 0) / fin.length;
    forward = `${finOffset > 0 ? '-' : '+'}${'XYZ'[k]}`;
    const confidence = Math.abs(finOffset) / ((bounds.max[k] - bounds.min[k]) / 2);
    notes.push(
      `forward ${forward}: asymmetry ${horizontal.map((h, i) => `${'XYZ'[h]}=${asym[i].toFixed(2)}`).join(' ')}; ` +
        `fin offset ${(confidence * 100).toFixed(0)} % of half length` +
        (confidence < 0.3 ? '  <-- UNSURE, check visually' : ''),
    );
  }
  if (axisIndex(forward) === axisIndex(up)) fail(`${entry.name}: forward and up share an axis`);
  return { forward, up, notes };
}

/// 0 = perfectly mirror-symmetric around the box centre along axis k.
function asymmetry(positions, bounds, k) {
  const bins = 64;
  const hist = new Array(bins).fill(0);
  const min = bounds.min[k];
  const size = bounds.max[k] - min || 1;
  for (const p of positions) hist[Math.min(bins - 1, Math.floor(((p[k] - min) / size) * bins))]++;
  let diff = 0;
  for (let i = 0; i < bins; i++) diff += Math.abs(hist[i] - hist[bins - 1 - i]);
  return diff / (2 * positions.length);
}

// --- Helpers ----------------------------------------------------------------

function collectPositions(nodes) {
  const out = [];
  for (const node of nodes) {
    for (const prim of node.getMesh().listPrimitives()) {
      const acc = prim.getAttribute('POSITION');
      for (let i = 0; i < acc.getCount(); i++) out.push(acc.getElement(i, []));
    }
  }
  return out;
}

function boundsOf(points) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (const p of points) {
    for (let i = 0; i < 3; i++) {
      min[i] = Math.min(min[i], p[i]);
      max[i] = Math.max(max[i], p[i]);
    }
  }
  return { min, max };
}

function axisIndex(axis) {
  const k = 'XYZ'.indexOf(axis.at(-1).toUpperCase());
  if (k < 0 || !['+', '-'].includes(axis[0])) fail(`bad axis "${axis}", expected e.g. "+X" or "-Z"`);
  return k;
}

function axisVector(axis) {
  const v = [0, 0, 0];
  v[axisIndex(axis)] = axis[0] === '-' ? -1 : 1;
  return v;
}

function extentAlong(bounds, v) {
  const k = v.findIndex((x) => x !== 0);
  return bounds.max[k] - bounds.min[k];
}

function dot(a, b) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

function cross(a, b) {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}

function parseLicense(text) {
  const field = (name) => text.match(new RegExp(`\\*\\s*${name}:\\s*(.+)`))?.[1].trim() ?? '';
  const [, title = ''] = field('title').match(/^(.*)$/) ?? [];
  const author = field('author');
  const license = field('license type').split(/\s+/)[0];
  return { title, url: field('source'), author, license, licenseUrl: field('license type').match(/\((.+)\)/)?.[1] ?? '' };
}

function formatReport(entry, report, out) {
  const dims = (b) => b.max.map((v, i) => (v - b.min[i]).toFixed(3)).join(' x ');
  const lines = [
    `== ${entry.name} -> ${path.relative(process.cwd(), out)}`,
    `   pose:    ${report.pose.chosen}` +
      (report.pose.candidates
        ? `  (animation "${report.pose.animation}"; ` +
          Object.entries(report.pose.candidates)
            .map(([k, c]) => `${k}: ${c.extents.map((v) => v.toFixed(2)).join('x')} tuck ${(c.tuck * 100).toFixed(0)} %`)
            .join(', ') +
          ')'
        : ''),
    `   axes:    forward ${report.axes.forward}, up ${report.axes.up}`,
    ...report.axes.notes.map((n) => `            ${n}`),
    `   before:  ${dims(report.before)} (source units, X x Y x Z)`,
    `   scale:   ${report.scale.toPrecision(6)}`,
    `   after:   ${dims(report.after)} m (X span x Y height x Z length)`,
    `   origin:  box min ${report.after.min.map((v) => v.toFixed(2)).join(', ')}  max ${report.after.max.map((v) => v.toFixed(2)).join(', ')}`,
    `   content: ${report.counts.meshes} meshes, ${report.counts.primitives} primitives, ${report.counts.textures} textures`,
  ];
  return lines.join('\n');
}

function formatCredits(credits) {
  const rows = credits.map(
    (c) => `- **${c.file}**: "${c.title}" (${c.url}) by ${c.author}, licensed under ${c.license} (${c.licenseUrl}). Rescaled, reoriented and recentred.`,
  );
  return `# Aircraft model credits\n\nThe models in this folder are based on the following works:\n\n${rows.join('\n')}\n`;
}

function parseArgs(list) {
  const out = {};
  for (let i = 0; i < list.length; i++) {
    if (!list[i].startsWith('--')) fail(`unexpected argument "${list[i]}"`);
    out[list[i].slice(2)] = list[i + 1];
    i++;
  }
  return out;
}

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}
