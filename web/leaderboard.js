// Hall of fame on the page: sends the game's scores to the online list and
// shows the top 10 for the chosen mode and level.
//
// The game (src/score/store.rs) writes to localStorage:
//   timefighter.top.<mode>.<level>  local top 10, one "name<TAB>score" line each
//   timefighter.outbox              JSON array of scores to send
// and reads back the answer to the last one sent:
//   timefighter.online              "id\nrank all\nrank week" and the top 10
//                                   like a local list, or "id\nerror\nreason"
// Names may hold any printable character (never a tab or line break); they
// are only ever shown with textContent.
// The online list's address comes from <meta name="tf-leaderboard">; when it
// is missing or empty, everything stays on this device.

const OUTBOX = 'timefighter.outbox';
const ONLINE = 'timefighter.online';

export const api = (document.querySelector('meta[name="tf-leaderboard"]')?.content ?? '')
  .trim()
  .replace(/\/+$/, '');

function storage() {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function localList(mode, level) {
  const text = storage()?.getItem(`timefighter.top.${mode}.${level}`) ?? '';
  // Lists from before names took any character: "NAME|score;...".
  const legacy = !text.includes('\t') && text.includes('|');
  const items = legacy
    ? text.split(';').map((item) => item.split('|'))
    : text.split('\n').map((line) => line.split('\t'));
  return items
    .filter(([name, score]) => name && /^\d+$/.test(score ?? ''))
    .map(([name, score]) => ({ name, score: Number(score) }));
}

/** Names from the server cannot hold tabs or line breaks; to be safe anyway. */
const flat = (name) => String(name).replace(/[\t\n\r]/g, ' ');
const asLines = (entries) => entries.map((e) => `${flat(e.name)}\t${e.score}`).join('\n');

async function send(item) {
  const { id, ...score } = item;
  const store = storage();
  if (!api) {
    store?.setItem(ONLINE, `${id}\nerror\nlocal only`);
    return;
  }
  try {
    const response = await fetch(`${api}/scores`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(score),
    });
    const answer = await response.json().catch(() => ({}));
    if (!response.ok) {
      store?.setItem(ONLINE, `${id}\nerror\n${answer.error ?? `server ${response.status}`}`);
      return;
    }
    const rank = (r) => (Number.isInteger(r) ? r : '-');
    store?.setItem(
      ONLINE,
      `${id}\n${rank(answer.rank_all)}\n${rank(answer.rank_week)}\n${asLines(answer.top ?? [])}`,
    );
  } catch {
    store?.setItem(ONLINE, `${id}\nerror\noffline`);
  }
}

/** Sends what the game queued, checking once a second. */
export function pumpOutbox() {
  setInterval(() => {
    const store = storage();
    const text = store?.getItem(OUTBOX);
    if (!text || text === '[]') return;
    store.setItem(OUTBOX, '[]');
    let items = [];
    try {
      items = JSON.parse(text);
    } catch {
      return;
    }
    for (const item of Array.isArray(items) ? items : []) send(item);
  }, 1000);
}

const cache = new Map();

async function onlineList(mode, level, period) {
  const key = `${mode}:${level}:${period}`;
  const hit = cache.get(key);
  if (hit && Date.now() - hit.at < 60_000) return hit.entries;
  const response = await fetch(`${api}/scores?mode=${mode}&level=${level}&period=${period}`);
  if (!response.ok) throw new Error(`server ${response.status}`);
  const { entries } = await response.json();
  cache.set(key, { at: Date.now(), entries });
  return entries;
}

/**
 * Wires the hall of fame box (<details id="hof">). Returns `show(mode,
 * level)` to call whenever the choice changes.
 */
export function hallOfFame() {
  const box = document.getElementById('hof');
  const list = document.getElementById('hof-list');
  const note = document.getElementById('hof-note');
  const title = document.getElementById('hof-title');
  const tabs = [...box.querySelectorAll('[data-board]')];
  let board = 'local';
  let current = { mode: 'hunt', level: 'normal', label: '' };

  for (const tab of tabs) {
    if (tab.dataset.board !== 'local' && !api) tab.hidden = true;
    tab.addEventListener('click', () => {
      board = tab.dataset.board;
      render();
    });
  }
  box.addEventListener('toggle', render);

  function fill(entries, empty) {
    list.replaceChildren(
      ...entries.slice(0, 10).map((entry) => {
        const item = document.createElement('li');
        const name = document.createElement('span');
        name.textContent = entry.name;
        const score = document.createElement('span');
        score.textContent = entry.score.toLocaleString('en');
        item.append(name, score);
        return item;
      }),
    );
    note.textContent = entries.length ? '' : empty;
  }

  async function render() {
    const { mode, level, label } = current;
    title.textContent = label;
    const online = board !== 'local' && api && level !== 'custom';
    for (const tab of tabs) {
      tab.setAttribute('aria-pressed', String(tab.dataset.board === board));
      tab.disabled = tab.dataset.board !== 'local' && level === 'custom';
    }
    if (!box.open) return;
    if (!online) {
      fill(localList(mode, level), level === 'custom' && board !== 'local'
        ? 'Custom scores stay on this device.'
        : 'No scores on this device yet.');
      return;
    }
    note.textContent = 'Loading…';
    try {
      const entries = await onlineList(mode, level, board);
      if (current.mode === mode && current.level === level) fill(entries, 'No scores yet.');
    } catch {
      fill([], 'The online list cannot be reached.');
    }
  }

  return function show(mode, level, label) {
    current = { mode, level, label };
    render();
  };
}
