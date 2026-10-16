# TimeFighter

A small arcade flight-combat game written in Rust with [Bevy](https://bevyengine.org), playable in the browser (WebAssembly).

A modern jet fighter is pulled through a time vortex over the Bermuda Triangle and lands in the middle of a World War II bomber raid. Defend the city with machine guns, guided missiles and bombs.

**Status:** early development.

## Controls

| Key | Action |
|---|---|
| W / S | pitch (W = nose down) |
| A / D | roll |
| Q / E | yaw |
| Up / Down | throttle |
| R | reset |

## Building

```sh
cargo test                  # headless tests, no window or GPU needed
cargo run                   # native build
scripts/build-web.sh        # browser build into dist/
```

The browser build needs `rustup target add wasm32-unknown-unknown` and
`wasm-bindgen-cli` in exactly the version pinned in `Cargo.lock`.

## Conventions

- 1 unit = 1 metre, +Y up, aircraft noses point along −Z.
- Gameplay logic lives in plugins that run headless, so it is tested
  with `MinimalPlugins` without a renderer.
- Bevy is pinned to an exact version.
