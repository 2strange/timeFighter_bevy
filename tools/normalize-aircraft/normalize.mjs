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
//                      [--out ../../assets/aircraft]
//                      [--library ../../models_library/aircraft] [--only <name>]
//
// Every manifest entry names a source folder (scene.gltf + license.txt),
// the real length in metres, and the source's forward/up axes ("+X", "-Z",
// ...). "auto" lets the script guess and print its reasoning; pin the guess
// in the manifest once it has been checked visually.
//
// "library": true writes the model to the library folder instead of the
// game's assets (kept for later use, not loaded or deployed). Each folder
// gets its own mounts.json and CREDITS.md.
//
// Optional per entry, for detailed models that must become light enough to
// fly in numbers:
//   "drop_materials": regex  - remove primitives whose material matches
//                              (e.g. cockpit interior, instruments)
//   "drop_nodes": regex      - remove meshes below nodes whose name matches
//   "target_triangles": n    - simplify (meshoptimizer) to about n triangles;
//                              normals are dropped first so faces can merge
//                              (the game computes flat normals, low-poly look)
//   "textures": [{ "material", "image", "size", "trim" }]
//                            - replace the base colour texture of materials
//                              whose name matches with an image (PNG or SVG,
//                              path relative to the manifest), rendered at
//                              size x size, with "trim" its transparent
//                              margins cut first; e.g. a tail emblem
//   "max_texture": px        - downscale textures to at most px per side and
//                              store them as PNG (the game decodes only PNG)
//   "mounts": [{ "name", "nodes" | "materials", "pick" }]
//                            - points of interest (gun turrets), measured in
//                              the normalised model: the centre of the
//                              matched geometry, or with "pick": "front" /
//                              "rear" / "top" / "bottom" the centre of its
//                              part within 1.5 m of that extreme; "z": [a, b]
//                              limits the search to that length range (m,
//                              nose at -Z), "x" and "y" likewise across and
//                              up (to single out e.g. an engine car on a
//                              one-mesh airship). Written to mounts.json.

import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { NodeIO, getBounds } from '@gltf-transform/core';
import {
  clearNodeTransform,
  dedup,
  flatten,
  join,
  prune,
  simplify,
  textureCompress,
  transformMesh,
  weld,
} from '@gltf-transform/functions';
import { MeshoptSimplifier } from 'meshoptimizer';
import sharp from 'sharp';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = parseArgs(process.argv.slice(2));
const manifestPath = path.resolve(args.manifest ?? path.join(here, '..', 'aircraft.json'));
const outDir = path.resolve(args.out ?? path.join(here, '..', '..', 'assets', 'aircraft'));
const libraryDir = path.resolve(
  args.library ?? path.join(here, '..', '..', 'models_library', 'aircraft'),
);
if (!args['source-root']) fail('missing --source-root <dir>');
const sourceRoot = path.resolve(args['source-root']);

// Only licences that allow use in a public, possibly commercial project.
const ALLOWED_LICENSES = ['CC-BY-4.0', 'CC0-1.0', 'CC0'];

const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
const io = new NodeIO();

// Per output folder: its credits and mount points.
const folders = new Map();
for (const dir of [outDir, libraryDir]) {
  await mkdir(dir, { recursive: true });
  const mountsPath = path.join(dir, 'mounts.json');
  const mounts = existsSync(mountsPath) ? JSON.parse(await readFile(mountsPath, 'utf8')) : {};
  folders.set(dir, { credits: [], mountsPath, mounts });
}
for (const entry of manifest.aircraft) {
  const dir = entry.library ? libraryDir : outDir;
  const { credits, mounts } = folders.get(dir);
  if (args.only && args.only !== entry.name) continue;
  const source = path.join(sourceRoot, entry.source);
  const license = parseLicense(await readFile(path.join(source, 'license.txt'), 'utf8'));
  if (!ALLOWED_LICENSES.includes(license.license)) {
    fail(`${entry.name}: licence "${license.license}" is not allowed (${ALLOWED_LICENSES.join(', ')})`);
  }
  credits.push({
    ...license,
    file: `${entry.name}.glb`,
    simplified: Boolean(entry.target_triangles),
    textures: (entry.textures ?? []).length > 0,
  });

  const doc = await io.read(path.join(source, 'scene.gltf'));
  const report = await normalize(doc, entry);
  const out = path.join(dir, `${entry.name}.glb`);
  await io.write(out, doc);
  console.log(formatReport(entry, report, out));
  if (Object.keys(report.mounts).length) mounts[entry.name] = report.mounts;
  else delete mounts[entry.name];
}

for (const [dir, { credits, mountsPath, mounts }] of folders) {
  await writeFile(mountsPath, JSON.stringify(mounts, null, 2) + '\n');
  if (!args.only) {
    await writeFile(path.join(dir, 'CREDITS.md'), formatCredits(credits));
    console.log(`credits: ${path.join(dir, 'CREDITS.md')}`);
  }
}

async function normalize(doc, entry) {
  const root = doc.getRoot();
  const scene = root.getDefaultScene() ?? root.listScenes()[0];

  const pose = choosePose(doc, scene, entry);
  for (const animation of root.listAnimations()) animation.dispose();
  await replaceTextures(root, entry);

  // Remember each mesh node's ancestry, so mounts and drops can match the
  // names of groups that flattening removes.
  const paths = new Map();
  const tag = (node, prefix) => {
    const here = prefix ? `${prefix}/${node.getName()}` : node.getName();
    paths.set(node, here);
    node.listChildren().forEach((child) => tag(child, here));
  };
  scene.listChildren().forEach((child) => tag(child, ''));
  const trianglesBefore = countTriangles(root);
  dropParts(root, scene, paths, entry);

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

  // Mount geometry is picked before simplifying (small parts such as gun
  // barrels may not survive it); the size is measured after, so the final
  // length matches what is drawn.
  const mountSources = collectMountPoints(meshNodes, paths, entry);
  // Simplifying trims the two wing tips unevenly; the symmetry plane is
  // taken from the full geometry.
  const unsimplified = boundsOf(collectPositions(meshNodes));
  if (entry.target_triangles) await simplifyTo(doc, entry.target_triangles);

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
  const symmetryX =
    [unsimplified.min, unsimplified.max].reduce((sum, p) => sum + dot(r, p) * scale, 0) / 2;
  const centre = [
    entry.target_triangles ? symmetryX : (box.min[0] + box.max[0]) / 2,
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
  const apply = (p) => [0, 1, 2].map((i) => m[i] * p[0] + m[4 + i] * p[1] + m[8 + i] * p[2] + m[12 + i]);
  const mountPoints = measureMounts(mountSources, apply, entry);

  await doc.transform(join({ keepNamed: false }), prune(), dedup());
  if (entry.max_texture) {
    await doc.transform(
      // PNG: the game only decodes PNG (no JPEG support compiled in).
      textureCompress({
        encoder: sharp,
        targetFormat: 'png',
        resize: [entry.max_texture, entry.max_texture],
      }),
    );
  }
  const after = getBounds(scene);
  const counts = {
    meshes: root.listMeshes().length,
    primitives: root.listMeshes().reduce((s, mesh) => s + mesh.listPrimitives().length, 0),
    textures: root.listTextures().length,
    trianglesBefore,
    triangles: countTriangles(root),
  };
  return { pose, axes, before, after, scale, counts, mounts: mountPoints };
}

// --- Detail: dropping, simplifying, mounts ------------------------------------

function countTriangles(root) {
  let n = 0;
  for (const mesh of root.listMeshes()) {
    for (const prim of mesh.listPrimitives()) {
      const indices = prim.getIndices();
      n += Math.floor((indices ?? prim.getAttribute('POSITION')).getCount() / 3);
    }
  }
  return n;
}

function dropParts(root, scene, paths, entry) {
  const byMaterial = entry.drop_materials && new RegExp(entry.drop_materials);
  const byNode = entry.drop_nodes && new RegExp(entry.drop_nodes);
  if (byNode) {
    scene.traverse((node) => {
      if (node.getMesh() && byNode.test(paths.get(node) ?? '')) node.setMesh(null);
    });
  }
  if (byMaterial) {
    for (const mesh of root.listMeshes()) {
      for (const prim of mesh.listPrimitives()) {
        if (byMaterial.test(prim.getMaterial()?.getName() ?? '')) prim.dispose();
      }
    }
  }
}

/// Swaps base colour textures for the manifest's images ("textures").
async function replaceTextures(root, entry) {
  for (const spec of entry.textures ?? []) {
    const pattern = new RegExp(spec.material);
    const file = path.resolve(path.dirname(manifestPath), spec.image);
    const size = spec.size ?? 256;
    let image = await sharp(await readFile(file), { density: 300 }).png().toBuffer();
    if (spec.trim) image = await sharp(image).trim().toBuffer();
    const png = await sharp(image)
      .resize(size, size, { fit: 'contain', background: { r: 0, g: 0, b: 0, alpha: 0 } })
      .png()
      .toBuffer();
    let replaced = 0;
    for (const material of root.listMaterials()) {
      const texture = material.getBaseColorTexture();
      if (!texture || !pattern.test(material.getName())) continue;
      texture.setImage(new Uint8Array(png)).setMimeType('image/png');
      replaced += 1;
    }
    if (!replaced) fail(`${entry.name}: no material matches "${spec.material}" for ${spec.image}`);
  }
}

/// Simplifies until the model has at most about `target` triangles, raising
/// the allowed error step by step so silhouettes keep what matters most.
async function simplifyTo(doc, target) {
  await MeshoptSimplifier.ready;
  // Per-face normals keep neighbouring triangles from sharing vertices,
  // which leaves the simplifier nothing to collapse.
  for (const mesh of doc.getRoot().listMeshes()) {
    for (const prim of mesh.listPrimitives()) prim.setAttribute('NORMAL', null);
  }
  await doc.transform(weld());
  for (const error of [0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2]) {
    const now = countTriangles(doc.getRoot());
    if (now <= target * 1.1) return;
    const ratio = Math.min(1, target / now);
    await doc.transform(simplify({ simplifier: MeshoptSimplifier, ratio, error }), prune());
  }
}

/// Source-space vertices of the geometry each mount names.
function collectMountPoints(meshNodes, paths, entry) {
  const result = [];
  for (const spec of entry.mounts ?? []) {
    const byNode = spec.nodes && new RegExp(spec.nodes);
    const byMaterial = spec.materials && new RegExp(spec.materials);
    const points = [];
    for (const node of meshNodes) {
      const mesh = node.getMesh();
      if (!mesh) continue;
      const nodeMatch = byNode && byNode.test(paths.get(node) ?? '');
      for (const prim of mesh.listPrimitives()) {
        const materialMatch = byMaterial && byMaterial.test(prim.getMaterial()?.getName() ?? '');
        if (!nodeMatch && !materialMatch) continue;
        const acc = prim.getAttribute('POSITION');
        for (let i = 0; i < acc.getCount(); i++) points.push(acc.getElement(i, []));
      }
    }
    if (!points.length) fail(`${entry.name}: mount "${spec.name}" matched no geometry`);
    result.push({ spec, points });
  }
  return result;
}

/// Mount positions in the normalised model (`apply` maps source to model).
function measureMounts(sources, apply, entry) {
  const result = {};
  for (const { spec, points: raw } of sources) {
    let points = raw.map(apply);
    for (const [k, axis] of ['x', 'y', 'z'].entries()) {
      const range = spec[axis];
      if (range) points = points.filter((p) => p[k] >= range[0] && p[k] <= range[1]);
    }
    if (!points.length) fail(`${entry.name}: mount "${spec.name}" has nothing in its range`);
    const axis = { front: [2, -1], rear: [2, 1], top: [1, 1], bottom: [1, -1] }[spec.pick];
    if (axis) {
      const [k, sign] = axis;
      const extreme = Math.max(...points.map((p) => p[k] * sign));
      points = points.filter((p) => extreme - p[k] * sign <= 1.5);
    }
    const box = boundsOf(points);
    result[spec.name] = box.min.map((v, i) => Math.round(((v + box.max[i]) / 2) * 100) / 100);
  }
  return result;
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
    for (const prim of node.getMesh()?.listPrimitives() ?? []) {
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
    `   triangles: ${report.counts.trianglesBefore} -> ${report.counts.triangles}`,
    ...Object.entries(report.mounts).map(
      ([name, p]) => `   mount:   ${name} at ${p.map((v) => v.toFixed(2)).join(', ')}`,
    ),
  ];
  return lines.join('\n');
}

function formatCredits(credits) {
  const rows = credits.map(
    (c) => `- **${c.file}**: "${c.title}" (${c.url}) by ${c.author}, licensed under ${c.license} (${c.licenseUrl}). Rescaled, reoriented and recentred${c.simplified ? '; simplified, interior and small parts reduced' : ''}${c.textures ? '; emblem replaced' : ''}.`,
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
