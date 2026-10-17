# Model library

Models that were prepared for the game but are not in it (any more). They
are kept because the work in them is done: each `.glb` is normalised the
same way as `assets/aircraft/` (1 unit = 1 m, +Y up, nose along −Z, origin
on the fuselage axis, textures embedded), so it can go straight back into
the game or into another engine (e.g. three.js).

Nothing here is loaded by the game or copied into the browser build.

- `aircraft/*.glb`: the models
- `aircraft/mounts.json`: measured gun positions (m, model frame)
- `aircraft/CREDITS.md`: sources and licences (CC BY 4.0 or CC0; keep the
  attribution when using a model)

`tools/normalize-aircraft` writes here for manifest entries marked
`"library": true` in `tools/aircraft.json`. To bring a model back into the
game, drop the flag, run the normaliser and add it to `AircraftModel`.
