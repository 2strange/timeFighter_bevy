// Touch controls for phones and tablets (pointer: coarse), as an HTML
// overlay over the game. The game reads one line of state from
// <html data-tf-input="…"> every frame (src/mobile/mod.rs) and tells the
// overlay what to show in <html data-tf-mode="…">: menu, play, paused,
// over or dialog (with the name to prefill in data-tf-name).
//
//   src=gyro|stick  p, r  pitch and roll: tilt in degrees from the
//                         recentred hold (gyro) or the stick, -1..1
//   t               thrust slider, 0..1
//   g               gun held (1/0)
//   m n x a M       taps as counters: missile, next target, pause,
//                   fly again, menu
//   name ok esc     the name field's text while the dialog is open, and
//                   its save and cancel taps
//
// Tilt: holding the phone in landscape, turning it like a steering wheel
// rolls, tipping the top edge away or towards you pitches. The angles come
// from the gravity direction in the screen's own frame, so they hold in
// either landscape orientation.

const coarse = matchMedia('(pointer: coarse)').matches;
const root = document.documentElement;

const state = {
  src: 'none',
  p: 0,
  r: 0,
  t: 0.55,
  g: 0,
  m: 0,
  n: 0,
  x: 0,
  a: 0,
  M: 0,
  name: null,
  ok: 0,
  esc: 0,
};

function publish() {
  const parts = [`src=${state.src}`, `p=${state.p.toFixed(2)}`, `r=${state.r.toFixed(2)}`];
  parts.push(`t=${state.t.toFixed(3)}`, `g=${state.g}`);
  for (const key of ['m', 'n', 'x', 'a', 'M', 'ok', 'esc']) parts.push(`${key}=${state[key]}`);
  if (state.name !== null) parts.push(`name=${encodeURIComponent(state.name)}`);
  root.dataset.tfInput = parts.join(';');
}

// --- Tilt -------------------------------------------------------------------

let baseline = null;
let latest = null;

/** Pitch and roll (degrees) of the device from beta/gamma and the screen angle. */
export function tiltAngles(beta, gamma, screenAngle) {
  const b = (beta * Math.PI) / 180;
  const g = (gamma * Math.PI) / 180;
  // Gravity in the device frame (x right, y up in portrait, z out of the screen).
  const gx = Math.cos(b) * Math.sin(g);
  const gy = -Math.sin(b);
  const gz = -Math.cos(b) * Math.cos(g);
  // In the screen's frame: right and up as the picture shows them.
  const a = (screenAngle * Math.PI) / 180;
  const right = Math.cos(a) * gx - Math.sin(a) * gy;
  const up = Math.sin(a) * gx + Math.cos(a) * gy;
  const deg = 180 / Math.PI;
  return {
    // Right side down: positive.
    roll: Math.atan2(right, -up) * deg,
    // Top edge tipped away (screen facing more upwards): positive.
    pitch: Math.atan2(-gz, -up) * deg,
  };
}

function screenAngle() {
  return screen.orientation?.angle ?? window.orientation ?? 0;
}

function onOrientation(event) {
  if (event.beta === null || event.gamma === null) return;
  latest = tiltAngles(event.beta, event.gamma, screenAngle());
  baseline ??= latest;
  const wrap = (d) => ((d + 540) % 360) - 180;
  state.src = 'gyro';
  state.p = wrap(latest.pitch - baseline.pitch);
  state.r = wrap(latest.roll - baseline.roll);
  publish();
}

/** The current hold becomes straight and level. */
export function recenter() {
  baseline = latest;
  state.p = 0;
  state.r = 0;
  publish();
}

// --- Overlay ----------------------------------------------------------------

const CSS = `
#tc { position: fixed; inset: 0; pointer-events: none; z-index: 5; display: none;
  font-family: "Orbitron", system-ui, sans-serif; color: #b8ffc7; user-select: none;
  -webkit-user-select: none; -webkit-touch-callout: none; }
body.playing #tc { display: block; }
#tc .group { display: none; }
html[data-tf-mode="play"] #tc .play, html[data-tf-mode="paused"] #tc .paused,
html[data-tf-mode="over"] #tc .over, html[data-tf-mode="dialog"] #tc .dialog { display: flex; }
#tc button, #tc .pad { pointer-events: auto; touch-action: none; font: inherit; color: inherit;
  background: rgba(6, 10, 14, 0.45); border: 2px solid rgba(184, 255, 199, 0.6); border-radius: 14px; }
#tc button:active, #tc .on { background: rgba(255, 191, 51, 0.35); border-color: #ffbf33; }
#tc .left { position: absolute; left: calc(14px + env(safe-area-inset-left)); bottom: 16px; top: 64px;
  flex-direction: column; justify-content: flex-end; gap: 14px; }
#tc .right { position: absolute; right: calc(14px + env(safe-area-inset-right)); top: 64px; bottom: 16px;
  flex-direction: column; justify-content: flex-start; align-items: flex-end; gap: 12px; }
#tc .top { position: absolute; top: 10px; left: 50%; transform: translateX(-50%); gap: 10px; }
#tc .slider { width: 64px; flex: 1; max-height: 240px; position: relative; }
#tc .slider .fill { position: absolute; left: 6px; right: 6px; bottom: 6px; border-radius: 9px;
  background: rgba(184, 255, 199, 0.35); }
#tc .slider span, #tc .stick span { position: absolute; left: 0; right: 0; top: 6px; text-align: center;
  font-size: 11px; letter-spacing: 0.1em; }
#tc .stick { width: 132px; height: 132px; border-radius: 50%; position: relative; display: none; }
#tc.no-gyro .stick { display: block; }
#tc.no-gyro .slider { max-height: 150px; }
#tc .stick .knob { position: absolute; width: 48px; height: 48px; left: 42px; top: 42px; border-radius: 50%;
  background: rgba(184, 255, 199, 0.5); }
#tc .gun { width: 104px; height: 104px; border-radius: 50%; font-size: 16px; }
#tc .missile { width: 84px; height: 84px; border-radius: 50%; font-size: 13px; }
#tc .slot { width: 64px; height: 64px; border-radius: 50%; opacity: 0.25; }
#tc .small { padding: 8px 14px; font-size: 12px; letter-spacing: 0.08em; }
#tc .center { position: absolute; left: 50%; bottom: 18px; transform: translateX(-50%); gap: 14px; }
#tc .center button { padding: 14px 26px; font-size: 16px; letter-spacing: 0.12em; }
#tc .dialog { position: absolute; left: 50%; top: 58%; transform: translateX(-50%); gap: 10px;
  align-items: center; }
#tc .dialog input { pointer-events: auto; width: min(60vw, 340px); padding: 12px 14px; font: 20px system-ui, sans-serif;
  color: #ffbf33; background: rgba(6, 10, 14, 0.9); border: 2px solid #ffbf33; border-radius: 10px; }
#tc .dialog button { padding: 12px 18px; }
#tc .turn { position: fixed; inset: 0; display: none; align-items: center; justify-content: center;
  background: rgba(6, 10, 14, 0.92); font-size: 18px; letter-spacing: 0.1em; pointer-events: auto; }
@media (orientation: portrait) { body.playing #tc .turn { display: flex; } }
`;

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

/** A button that counts its taps into state[key]. */
function tapButton(className, text, key) {
  const button = el('button', className, text);
  button.type = 'button';
  button.addEventListener('pointerdown', (event) => {
    event.preventDefault();
    state[key] += 1;
    publish();
  });
  return button;
}

function build() {
  const style = el('style');
  style.textContent = CSS;
  document.head.append(style);

  const overlay = el('div');
  overlay.id = 'tc';

  // Left: thrust slider, and the stick when there is no gyro.
  const left = el('div', 'group play left');
  const slider = el('div', 'pad slider');
  const fill = el('div', 'fill');
  slider.append(fill, el('span', '', 'THRUST'));
  const showThrust = () => (fill.style.height = `calc(${state.t * 100}% - 12px)`);
  const setThrust = (event) => {
    const box = slider.getBoundingClientRect();
    state.t = Math.min(1, Math.max(0, 1 - (event.clientY - box.top) / box.height));
    showThrust();
    publish();
  };
  slider.addEventListener('pointerdown', (event) => {
    slider.setPointerCapture(event.pointerId);
    setThrust(event);
  });
  slider.addEventListener('pointermove', (event) => {
    if (slider.hasPointerCapture(event.pointerId)) setThrust(event);
  });
  showThrust();

  const stick = el('div', 'pad stick');
  const knob = el('div', 'knob');
  stick.append(knob, el('span', '', 'STICK'));
  const moveStick = (event) => {
    const box = stick.getBoundingClientRect();
    const radius = box.width / 2;
    let x = (event.clientX - box.left - radius) / radius;
    let y = (event.clientY - box.top - radius) / radius;
    const length = Math.hypot(x, y);
    if (length > 1) {
      x /= length;
      y /= length;
    }
    state.src = 'stick';
    state.r = x;
    state.p = -y;
    knob.style.transform = `translate(${x * (radius - 24)}px, ${y * (radius - 24)}px)`;
    publish();
  };
  stick.addEventListener('pointerdown', (event) => {
    stick.setPointerCapture(event.pointerId);
    moveStick(event);
  });
  stick.addEventListener('pointermove', (event) => {
    if (stick.hasPointerCapture(event.pointerId)) moveStick(event);
  });
  const releaseStick = () => {
    state.p = 0;
    state.r = 0;
    knob.style.transform = '';
    publish();
  };
  stick.addEventListener('pointerup', releaseStick);
  stick.addEventListener('pointercancel', releaseStick);
  left.append(stick, slider);

  // Right: gun, missile, a slot for a third weapon, next target.
  const right = el('div', 'group play right');
  const gun = el('button', 'gun', 'GUN');
  gun.type = 'button';
  const hold = (on) => (event) => {
    event.preventDefault();
    state.g = on ? 1 : 0;
    gun.classList.toggle('on', on);
    publish();
  };
  gun.addEventListener('pointerdown', (event) => {
    gun.setPointerCapture(event.pointerId);
    hold(true)(event);
  });
  for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) {
    gun.addEventListener(type, hold(false));
  }
  const slot = el('button', 'slot');
  slot.type = 'button';
  slot.disabled = true;
  slot.setAttribute('aria-hidden', 'true');
  right.append(gun, tapButton('missile', 'MISSILE', 'm'), slot, tapButton('small', 'TARGET', 'n'));

  // Top: pause and recenter.
  const top = el('div', 'group play top');
  const center = el('button', 'small', 'RECENTER');
  center.type = 'button';
  center.addEventListener('pointerdown', (event) => {
    event.preventDefault();
    recenter();
  });
  top.append(tapButton('small', 'PAUSE', 'x'), center);

  const paused = el('div', 'group paused center');
  paused.append(tapButton('', 'RESUME', 'x'));

  const over = el('div', 'group over center');
  over.append(tapButton('', 'FLY AGAIN', 'a'), tapButton('', 'MENU', 'M'));

  // The name: a real field, so the phone's keyboard opens when it is tapped.
  const dialog = el('div', 'group dialog');
  const field = el('input');
  field.maxLength = 18;
  field.autocomplete = 'off';
  field.setAttribute('enterkeyhint', 'done');
  field.placeholder = 'Your name';
  field.addEventListener('input', () => {
    state.name = field.value;
    publish();
  });
  const save = () => {
    state.name = field.value;
    state.ok += 1;
    field.blur();
    publish();
  };
  field.addEventListener('keydown', (event) => {
    if (event.key === 'Enter') save();
  });
  const ok = el('button', '', 'SAVE');
  ok.type = 'button';
  ok.addEventListener('click', save);
  const cancel = tapButton('', '✕', 'esc');
  dialog.append(field, ok, cancel);

  const turn = el('div', 'turn', 'TURN YOUR PHONE SIDEWAYS');
  overlay.append(left, right, top, paused, over, dialog, turn);
  document.body.append(overlay);

  // The game says what it shows.
  let mode = null;
  new MutationObserver(() => {
    const now = root.dataset.tfMode;
    if (now === mode) return;
    mode = now;
    if (mode === 'dialog') {
      field.value = root.dataset.tfName ?? '';
      state.name = field.value;
    } else {
      state.name = null;
      if (state.g) hold(false)(new Event('pointerup'));
    }
    publish();
  }).observe(root, { attributes: true, attributeFilter: ['data-tf-mode'] });

  return overlay;
}

let overlay = null;

/**
 * Called from the start button's click (a user gesture): asks for motion
 * access on iOS, goes full screen and locks landscape where the browser
 * allows. Does nothing on devices with a mouse.
 */
export function startTouch() {
  if (!coarse) return;
  overlay ??= build();
  document.body.classList.add('playing');
  publish();

  const element = document.documentElement;
  if (element.requestFullscreen && !document.fullscreenElement) {
    element
      .requestFullscreen({ navigationUI: 'hide' })
      .then(() => screen.orientation?.lock?.('landscape'))
      .catch(() => {});
  }

  const noGyro = () => {
    overlay.classList.add('no-gyro');
    state.src = 'stick';
    publish();
  };
  const listen = () => {
    window.addEventListener('deviceorientation', onOrientation);
    // No reading within a second: no gyro after all.
    setTimeout(() => {
      if (latest === null) noGyro();
    }, 1000);
  };
  const Orientation = window.DeviceOrientationEvent;
  if (!Orientation) {
    noGyro();
  } else if (typeof Orientation.requestPermission === 'function') {
    // iOS: must be asked within the tap.
    Orientation.requestPermission()
      .then((answer) => (answer === 'granted' ? listen() : noGyro()))
      .catch(noGyro);
  } else {
    listen();
  }
}

/** On an iPhone in Safari: a hint to add the game to the home screen. */
export function homeScreenHint(element) {
  const iphone = /iPhone|iPod/.test(navigator.userAgent);
  let dismissed = false;
  try {
    dismissed = localStorage.getItem('timefighter.homehint') === 'no';
  } catch {
    // No storage: show it.
  }
  if (!iphone || navigator.standalone || dismissed || !element) return;
  element.hidden = false;
  element.querySelector('button')?.addEventListener('click', () => {
    element.hidden = true;
    try {
      localStorage.setItem('timefighter.homehint', 'no');
    } catch {
      // Shown again next time.
    }
  });
}
