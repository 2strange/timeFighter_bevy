# render-aircraft

Renders still images of the normalised aircraft (`assets/aircraft/*.glb`) on
the CPU, without GPU or browser: a small rasteriser with depth buffer,
perspective-correct textures and flat shading, supersampled, written as WebP
with a transparent background. Used for the mode pictures on the start page.

```sh
npm ci
node render.mjs            # scenes.json -> ../../web/img/<name>.webp
node render.mjs --only mode-hunt
```

`scenes.json` places models (position in metres, yaw/pitch/roll in degrees)
and sets the view angle; the camera frames the result automatically.
