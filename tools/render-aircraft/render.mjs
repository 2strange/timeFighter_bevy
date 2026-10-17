#!/usr/bin/env node
// Renders still images of the normalised aircraft models on the CPU: no GPU,
// no browser. A small rasteriser with a depth buffer, perspective-correct
// texture lookup and flat Lambert shading (the game's low-poly look),
// supersampled and written as WebP with a transparent background, or as
// JPG/PNG composited between a background and an overlay SVG (share images).
//
// Usage:
//   node render.mjs [--scenes scenes.json] [--assets ../../assets/aircraft]
//                   [--out ../../web/img] [--only <name>]
//
// scenes.json: { "<name>": { "width", "height", "view": { "yaw", "pitch" },
//   "aircraft": [{ "model", "at": [x,y,z], "yaw", "pitch", "roll" }] } }
// Angles in degrees; the camera frames all aircraft from the view angles.
// Optional per scene:
//   "camera": { "eye": [x,y,z], "target": [x,y,z], "fov" }  fixed camera, no framing
//   "light": { "dir": [x,y,z], "color": [r,g,b], "ambient", "sky": [r,g,b] }
//   "haze": { "color": [r,g,b], "near", "far", "max" }      fades distant faces
//   "background", "overlay": SVG files (next to scenes.json) under/over the planes
//   "format": "webp" | "jpg" | "png", "quality"
//   "also": [{ "name", "width", "height", "format" }]        resized copies
// SVG text uses only the fonts in ./fonts (Orbitron, OFL).

import { readFile, mkdir } from 'node:fs/promises';
import { writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { NodeIO } from '@gltf-transform/core';
import sharp from 'sharp';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = parseArgs(process.argv.slice(2));
const scenesPath = path.resolve(args.scenes ?? path.join(here, 'scenes.json'));
const assetDir = path.resolve(args.assets ?? path.join(here, '..', '..', 'assets', 'aircraft'));
const outDir = path.resolve(args.out ?? path.join(here, '..', '..', 'web', 'img'));
const SUPERSAMPLE = 3;
const LIGHT = { dir: [-0.45, 0.75, -0.5], color: [1, 1, 1], ambient: 0.42, sky: [1, 1, 1] };

// Point fontconfig (used by librsvg for SVG text) at the bundled fonts only,
// so the share image looks the same on every machine.
const fontConfig = path.join(os.tmpdir(), 'render-aircraft-fonts.conf');
writeFileSync(
  fontConfig,
  `<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd"><fontconfig>` +
    `<dir>${path.join(here, 'fonts')}</dir>` +
    `<cachedir>${path.join(os.tmpdir(), 'render-aircraft-fontcache')}</cachedir></fontconfig>`,
);
process.env.FONTCONFIG_FILE = fontConfig;

const scenes = JSON.parse(await readFile(scenesPath, 'utf8'));
await mkdir(outDir, { recursive: true });
const io = new NodeIO();
const models = new Map();

for (const [name, scene] of Object.entries(scenes)) {
  if (args.only && args.only !== name) continue;
  const triangles = [];
  for (const placed of scene.aircraft) {
    const model = await loadModel(placed.model);
    const pose = rotation(placed.yaw ?? 0, placed.pitch ?? 0, placed.roll ?? 0);
    for (const t of model) {
      triangles.push({
        p: t.p.map((v) => add(mulMat(pose, v), placed.at)),
        uv: t.uv,
        texture: t.texture,
        color: t.color,
      });
    }
  }
  const image = render(triangles, scene);
  let picture = sharp(image.data, { raw: { width: image.width, height: image.height, channels: 4 } })
    .resize(scene.width, scene.height, { kernel: 'lanczos3' });
  if (scene.background || scene.overlay) {
    const planes = await picture.png().toBuffer();
    const base = scene.background
      ? sharp(await readFile(path.join(path.dirname(scenesPath), scene.background)))
      : sharp({ create: { width: scene.width, height: scene.height, channels: 4, background: '#0000' } });
    const layers = [{ input: planes }];
    if (scene.overlay) layers.push({ input: await readFile(path.join(path.dirname(scenesPath), scene.overlay)) });
    picture = sharp(await base.resize(scene.width, scene.height).composite(layers).png().toBuffer());
  }
  await write(picture, name, scene.format, scene.quality, `${triangles.length} triangles`);
  for (const copy of scene.also ?? []) {
    await write(picture.clone().resize(copy.width, copy.height, { kernel: 'lanczos3' }), copy.name, copy.format, copy.quality, `copy of ${name}`);
  }
}

async function write(picture, name, format = 'webp', quality, note) {
  const ext = format === 'jpg' ? 'jpg' : format;
  const file = path.join(outDir, `${name}.${ext}`);
  if (format === 'jpg') await picture.jpeg({ quality: quality ?? 82, optimiseCoding: true, trellisQuantisation: true }).toFile(file);
  else if (format === 'png') await picture.png({ compressionLevel: 9, palette: false }).toFile(file);
  else await picture.webp({ quality: quality ?? 82, alphaQuality: 90 }).toFile(file);
  const size = (await readFile(file)).length;
  console.log(`${name}: ${note} -> ${path.relative(process.cwd(), file)} (${(size / 1024).toFixed(1)} kB)`);
}

// --- Models -------------------------------------------------------------------

async function loadModel(name) {
  if (models.has(name)) return models.get(name);
  const doc = await io.read(path.join(assetDir, `${name}.glb`));
  const textures = new Map();
  const triangles = [];
  for (const node of doc.getRoot().listNodes()) {
    const mesh = node.getMesh();
    if (!mesh) continue;
    const world = node.getWorldMatrix();
    for (const prim of mesh.listPrimitives()) {
      const material = prim.getMaterial();
      const factor = material?.getBaseColorFactor() ?? [1, 1, 1, 1];
      const image = material?.getBaseColorTexture();
      let texture = null;
      if (image) {
        if (!textures.has(image)) textures.set(image, await decode(image.getImage()));
        texture = textures.get(image);
      }
      const pos = prim.getAttribute('POSITION');
      const uvs = prim.getAttribute('TEXCOORD_0');
      const idx = prim.getIndices();
      const count = idx ? idx.getCount() : pos.getCount();
      const at = (i) => (idx ? idx.getScalar(i) : i);
      for (let i = 0; i + 2 < count; i += 3) {
        const corners = [at(i), at(i + 1), at(i + 2)];
        triangles.push({
          p: corners.map((c) => transformPoint(world, pos.getElement(c, []))),
          uv: uvs ? corners.map((c) => uvs.getElement(c, [])) : null,
          texture,
          color: factor,
        });
      }
    }
  }
  models.set(name, triangles);
  return triangles;
}

async function decode(bytes) {
  const { data, info } = await sharp(Buffer.from(bytes)).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  return { data, width: info.width, height: info.height };
}

function sample(texture, u, v) {
  // Bilinear, repeating.
  const x = (u - Math.floor(u)) * texture.width - 0.5;
  const y = (v - Math.floor(v)) * texture.height - 0.5;
  const x0 = Math.floor(x);
  const y0 = Math.floor(y);
  const fx = x - x0;
  const fy = y - y0;
  const px = (xx, yy) => {
    const wx = ((xx % texture.width) + texture.width) % texture.width;
    const wy = ((yy % texture.height) + texture.height) % texture.height;
    const o = (wy * texture.width + wx) * 4;
    return [texture.data[o], texture.data[o + 1], texture.data[o + 2], texture.data[o + 3]];
  };
  const a = px(x0, y0);
  const b = px(x0 + 1, y0);
  const c = px(x0, y0 + 1);
  const d = px(x0 + 1, y0 + 1);
  return [0, 1, 2, 3].map(
    (k) => (a[k] * (1 - fx) + b[k] * fx) * (1 - fy) + (c[k] * (1 - fx) + d[k] * fx) * fy,
  );
}

// --- Rendering ----------------------------------------------------------------

function render(triangles, scene) {
  const width = scene.width * SUPERSAMPLE;
  const height = scene.height * SUPERSAMPLE;
  const fov = ((scene.camera?.fov ?? scene.fov ?? 30) * Math.PI) / 180;
  const sun = { ...LIGHT, ...scene.light };
  const toLight = normalize(sun.dir);
  const haze = scene.haze;

  // Frame everything: bounding sphere seen from the view direction.
  let lo = [Infinity, Infinity, Infinity];
  let hi = [-Infinity, -Infinity, -Infinity];
  for (const t of triangles) {
    for (const p of t.p) {
      lo = lo.map((v, i) => Math.min(v, p[i]));
      hi = hi.map((v, i) => Math.max(v, p[i]));
    }
  }
  let centre = lo.map((v, i) => (v + hi[i]) / 2);
  const radius = Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) / 2;
  const yaw = ((scene.view?.yaw ?? 35) * Math.PI) / 180;
  const pitch = ((scene.view?.pitch ?? 20) * Math.PI) / 180;
  const direction = [
    Math.sin(yaw) * Math.cos(pitch),
    Math.sin(pitch),
    -Math.cos(yaw) * Math.cos(pitch),
  ];
  const forward = scene.camera
    ? normalize(sub(scene.camera.target, scene.camera.eye))
    : direction.map((v) => -v);
  const right = normalize(cross(forward, [0, 1, 0]));
  const up = cross(right, forward);
  const focal = height / 2 / Math.tan(fov / 2);
  const margin = scene.margin ?? 0.9;

  // Start from the bounding sphere, then tighten: move the camera so the
  // projected outline fills the frame (up to `margin`) and is centred.
  let distance = radius / Math.sin(fov / 2);
  let eye = scene.camera ? scene.camera.eye : add(centre, direction.map((v) => v * distance));
  const corners = triangles.flatMap((t) => t.p);
  for (let pass = 0; pass < (scene.camera ? 0 : 12); pass++) {
    let [minX, maxX, minY, maxY] = [Infinity, -Infinity, Infinity, -Infinity];
    for (const p of corners) {
      const d = sub(p, eye);
      const z = dot(d, forward);
      const x = (dot(d, right) * focal) / z;
      const y = (dot(d, up) * focal) / z;
      minX = Math.min(minX, x);
      maxX = Math.max(maxX, x);
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y);
    }
    const fill = Math.max((maxX - minX) / width, (maxY - minY) / height) / margin;
    // Shift the target by the outline's offset, at the target's depth.
    const shift = add(
      right.map((v) => (v * ((minX + maxX) / 2) * distance) / focal),
      up.map((v) => (v * ((minY + maxY) / 2) * distance) / focal),
    );
    centre = add(centre, shift);
    distance *= fill;
    eye = add(centre, direction.map((v) => v * distance));
  }

  const project = (p) => {
    const d = sub(p, eye);
    const z = dot(d, forward);
    return [width / 2 + (dot(d, right) * focal) / z, height / 2 - (dot(d, up) * focal) / z, z];
  };

  const color = new Float32Array(width * height * 4);
  const depth = new Float32Array(width * height).fill(Infinity);

  for (const t of triangles) {
    const s = t.p.map(project);
    if (s.some((v) => v[2] <= 0.1)) continue;
    // Flat shading with the face normal turned towards the camera.
    let n = normalize(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
    if (dot(n, sub(eye, t.p[0])) < 0) n = n.map((v) => -v);
    const diffuse = (1 - sun.ambient) * Math.max(0, dot(n, toLight));
    const light = [0, 1, 2].map((k) => sun.ambient * sun.sky[k] + diffuse * sun.color[k]);

    const minX = Math.max(0, Math.floor(Math.min(s[0][0], s[1][0], s[2][0])));
    const maxX = Math.min(width - 1, Math.ceil(Math.max(s[0][0], s[1][0], s[2][0])));
    const minY = Math.max(0, Math.floor(Math.min(s[0][1], s[1][1], s[2][1])));
    const maxY = Math.min(height - 1, Math.ceil(Math.max(s[0][1], s[1][1], s[2][1])));
    const area = edge(s[0], s[1], s[2]);
    if (Math.abs(area) < 1e-9) continue;
    for (let y = minY; y <= maxY; y++) {
      for (let x = minX; x <= maxX; x++) {
        const p = [x + 0.5, y + 0.5];
        const w0 = edge(s[1], s[2], p) / area;
        const w1 = edge(s[2], s[0], p) / area;
        const w2 = edge(s[0], s[1], p) / area;
        if (w0 < 0 || w1 < 0 || w2 < 0) continue;
        // Perspective-correct interpolation via 1/z.
        const iz = w0 / s[0][2] + w1 / s[1][2] + w2 / s[2][2];
        const z = 1 / iz;
        const o = y * width + x;
        if (z >= depth[o]) continue;
        let rgba = t.color.map((c) => c * 255);
        if (t.texture && t.uv) {
          const u = (w0 * t.uv[0][0] / s[0][2] + w1 * t.uv[1][0] / s[1][2] + w2 * t.uv[2][0] / s[2][2]) * z;
          const v = (w0 * t.uv[0][1] / s[0][2] + w1 * t.uv[1][1] / s[1][2] + w2 * t.uv[2][1] / s[2][2]) * z;
          const texel = sample(t.texture, u, v);
          rgba = rgba.map((c, k) => (c * texel[k]) / 255);
        }
        if (rgba[3] < 128) continue;
        depth[o] = z;
        const fade = haze ? haze.max * Math.min(1, Math.max(0, (z - haze.near) / (haze.far - haze.near))) : 0;
        for (let k = 0; k < 3; k++) {
          color[o * 4 + k] = rgba[k] * light[k] * (1 - fade) + (haze ? haze.color[k] * fade : 0);
        }
        color[o * 4 + 3] = 255;
      }
    }
  }
  const data = Buffer.alloc(width * height * 4);
  for (let i = 0; i < data.length; i++) data[i] = Math.max(0, Math.min(255, Math.round(color[i])));
  return { data, width, height };
}

// --- Maths --------------------------------------------------------------------

function edge(a, b, p) {
  return (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
}

/// Model rotation: yaw about Y, then pitch about X, then roll about Z
/// (degrees, model frame: nose -Z, +Y up).
function rotation(yawDeg, pitchDeg, rollDeg) {
  const [y, p, r] = [yawDeg, pitchDeg, rollDeg].map((d) => (d * Math.PI) / 180);
  const ry = [[Math.cos(y), 0, Math.sin(y)], [0, 1, 0], [-Math.sin(y), 0, Math.cos(y)]];
  const rx = [[1, 0, 0], [0, Math.cos(p), -Math.sin(p)], [0, Math.sin(p), Math.cos(p)]];
  const rz = [[Math.cos(r), -Math.sin(r), 0], [Math.sin(r), Math.cos(r), 0], [0, 0, 1]];
  return mul3(mul3(ry, rx), rz);
}

function mul3(a, b) {
  return a.map((row) => [0, 1, 2].map((j) => row[0] * b[0][j] + row[1] * b[1][j] + row[2] * b[2][j]));
}

function mulMat(m, v) {
  return m.map((row) => row[0] * v[0] + row[1] * v[1] + row[2] * v[2]);
}

function transformPoint(m, p) {
  return [0, 1, 2].map((i) => m[i] * p[0] + m[4 + i] * p[1] + m[8 + i] * p[2] + m[12 + i]);
}

function add(a, b) {
  return a.map((v, i) => v + b[i]);
}

function sub(a, b) {
  return a.map((v, i) => v - b[i]);
}

function dot(a, b) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

function cross(a, b) {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}

function normalize(v) {
  const l = Math.hypot(...v) || 1;
  return v.map((x) => x / l);
}

function parseArgs(list) {
  const out = {};
  for (let i = 0; i < list.length; i += 2) out[list[i].replace(/^--/, '')] = list[i + 1];
  return out;
}
