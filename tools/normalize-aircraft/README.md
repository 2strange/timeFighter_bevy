# normalize-aircraft

One-off import step for aircraft models. Reads glTF models (with
`license.txt`), and writes one `.glb` per model to `assets/aircraft/` in the
game's conventions:

- 1 unit = 1 m, scaled to the real aircraft length
- +Y up, nose along −Z
- origin at the bounding-box centre in X/Z, on the fuselage axis in Y
- all node transforms baked into the vertices, compatible primitives joined,
  animations dropped (a keyframe pose can be baked first, e.g. gear up)
- textures embedded

Detailed models can be made light enough to fly in numbers: drop parts by
material or node name (cockpit interiors), simplify to a triangle budget
(meshoptimizer; normals are dropped so faces can merge, the game shades
them flat), and shrink textures (stored as PNG). Gun positions and other
points of interest are measured on the normalised model and written to
`assets/aircraft/mounts.json` (see the comment at the top of
`normalize.mjs` for all manifest options).

It also writes `assets/aircraft/CREDITS.md` from the licence files and
refuses any licence that is not on its allow-list.

```sh
npm ci
node normalize.mjs --source-root <folder with the source models>
```

The source root holds one folder per manifest `source` (e.g.
`historic/spitfire`, `inbox/b17`); downloaded archives are extracted there
first. Nothing in a source folder is executed, only read.

The manifest `../aircraft.json` lists per model: source folder, real length,
source forward/up axis (e.g. `"+X"`, `"-Z"`) and pose (`rest`, `first`,
`last` keyframe). Use `"auto"` for a new model: the script guesses (height =
smallest extent; length axis = the one that is not mirror-symmetric; tail =
where the highest vertices are; pose = landing gear tucked in), prints its
reasoning, and flags uncertain guesses. Pin the result in the manifest after
checking it in the in-game showroom (key M).
