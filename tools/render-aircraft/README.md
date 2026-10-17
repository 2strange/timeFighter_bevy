# render-aircraft

Renders still images of the normalised aircraft (`assets/aircraft/*.glb`) on
the CPU, without GPU or browser: a small rasteriser with depth buffer,
perspective-correct textures and flat shading, supersampled, written as WebP
with a transparent background. Used for the mode pictures on the start page,
and for the share preview (`og-image.jpg`, 1200x630) plus the touch icon and
favicon, which are composited between an SVG background and an SVG overlay.

```sh
npm ci
node render.mjs            # scenes.json -> ../../web/img/<name>.<webp|jpg|png>
node render.mjs --only og-image
```

`scenes.json` places models (position in metres, yaw/pitch/roll in degrees)
and sets the view angle; the camera frames the result automatically.

A scene can also fix the camera (`camera`), set sun and haze (`light`,
`haze`), add `background`/`overlay` SVGs and choose `format` (`webp`, `jpg`,
`png`); see the header of `render.mjs`. The share image keeps the wordmark,
tagline and the F-14 inside the centre 630x630, because some messengers crop
it to a square.

SVG text uses only the fonts in `fonts/`: Orbitron (variable TTF from
google/fonts), SIL Open Font License 1.1, see `fonts/OFL.txt`.
