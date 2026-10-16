TimeFighter - browser build
===========================

Browsers refuse to load WebAssembly from file://, so serve this folder
with any static web server on your own machine, for example:

    cd <this folder>
    python3 -m http.server 8000

Then open http://localhost:8000 in Chrome, Firefox or Safari and click
into the game once so it receives the keyboard.

Controls
    W / S         pitch (W = nose down, S = nose up)
    A / D         roll left / right
    Q / E         yaw left / right
    Left / Right  yaw left / right (same as Q / E)
    Up / Down     throttle (50 ... 500 m/s)
    R             reset to spawn
    P / Esc       pause (also automatic when the tab loses focus)

Aircraft showroom: http://localhost:8000/showroom.html
(drag with the mouse to orbit, scroll to zoom; or A/D, W/S, Q/E)

Stop the server with Ctrl+C when you are done.
