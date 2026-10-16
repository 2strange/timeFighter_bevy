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

It also writes `assets/aircraft/CREDITS.md` from the licence files and
refuses any licence that is not on its allow-list.

```sh
npm ci
node normalize.mjs --source-root <folder with the source models>
```

The manifest `../aircraft.json` lists per model: source folder, real length,
source forward/up axis (e.g. `"+X"`, `"-Z"`) and pose (`rest`, `first`,
`last` keyframe). Use `"auto"` for a new model: the script guesses (height =
smallest extent; length axis = the one that is not mirror-symmetric; tail =
where the highest vertices are; pose = landing gear tucked in), prints its
reasoning, and flags uncertain guesses. Pin the result in the manifest after
checking it in the in-game showroom (key M).
