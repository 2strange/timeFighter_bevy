# TimeFighter

An arcade flight-combat game built with Rust and [Bevy](https://bevyengine.org), playable in the browser through WebAssembly.

**Play:** [timefighter.trendgegner.com](https://timefighter.trendgegner.com)

## Story

A modern jet takes off from its air base, flies into a strange storm, and is struck by a flash of lightning. It emerges in a parallel timeline where the neighbouring city is under attack by propeller-era bombers and fighters.

## Modes and difficulty

- **Hunt:** playable now.
- **Protect:** coming soon.
- **Battle:** coming soon.

Choose from Super Easy, Easy, Normal, Hard, Insane, or Custom. Custom mode lets you tune missiles, enemy count, flight size, enemy speed and accuracy, and unlimited machine-gun ammunition.

## Controls

| Key | Action |
| --- | --- |
| W / S | Pitch (W = nose down) |
| A / D | Roll |
| Q / E or Left / Right | Yaw |
| Up / Down | Throttle |
| Space / left mouse button | Machine gun |
| Right Shift / right mouse button | Missile (homes on a locked target) |
| Tab or Right Alt (Option) | Next target (also automatic after a missile launch) |
| Enter or R | Fly again after a round |
| M | Return to the start menu after a round |
| P / Esc | Pause (also when the window loses focus) |
| I | Show or hide key help (shown briefly when a round starts) |

## How it was built

The game idea, architecture, and final sign-off are by Torsten Wetzel ([2strange](https://github.com/2strange) / [trendgegner](https://trendgegner.com)). The code was written by his team of Claude and Codex AI agents working in parallel, with cross-reviews and headless tests: Bob (lead), Maverick (flight, physics, build), Iceman (world), Goose (combat, AI, HUD), Marvin (tests, CI), and Riker (review).

## Architecture

The game is split into Bevy ECS plugins for flight, camera, Rapier physics with continuous collision detection (CCD), world and city generation, combat, AI, HUD, game state, and menus. One unit is one metre, Y is up, and aircraft point nose-first along −Z. Models are normalised once during import with `tools/normalize-aircraft`; visuals and collision shapes remain separate. Gameplay logic is covered by headless tests using `MinimalPlugins`.

## Building locally

Install the `wasm32-unknown-unknown` target and the `wasm-bindgen-cli` version pinned in `Cargo.lock`, then run:

```sh
scripts/build-web.sh
```

Serve `dist/` with any static-file server.

## License

The code is available under the [MIT License](LICENSE). Aircraft models are licensed separately under CC-BY-4.0; see the [aircraft credits](assets/aircraft/CREDITS.md) for authors and sources. Sounds are CC0; see the [sound credits](assets/audio/CREDITS.md).
