// Pilot names: the game's alphabet (src/score/board.rs) and a short list of
// words that do not go on a public list.

const ALLOWED = /^[A-Z0-9_-]+( [A-Z0-9_-]+)*$/;

// Checked against the name without spaces, dashes and underscores, with
// digits read as the letters they stand in for.
const BLOCKED = [
  'NAZI',
  'HITLER',
  'HEIL',
  'SIEG',
  'FUHRER',
  'FUEHRER',
  'HOLOCAUST',
  'KKK',
  'FUCK',
  'SHIT',
  'CUNT',
  'BITCH',
  'WHORE',
  'NIGG',
  'NEGER',
  'FAGG',
  'FOTZE',
  'HURE',
  'WICHS',
  'SCHWUCHTEL',
  'SPAST',
  'PENIS',
  'VAGINA',
];
// Number codes of the far right, checked on the raw name.
const BLOCKED_CODES = ['88', '1488'];
const LEET = { 0: 'O', 1: 'I', 3: 'E', 4: 'A', 5: 'S', 7: 'T', 8: 'B' };

/** A reason why `name` cannot go on the list, or null. */
export function checkName(name) {
  if (typeof name !== 'string') return 'no name';
  if (name.length < 3 || name.length > 12) return 'name must have 3 to 12 characters';
  if (!ALLOWED.test(name)) return 'name may use A-Z, 0-9, space, - and _';
  const squeezed = name.replace(/[ _-]/g, '');
  const read = squeezed.replace(/[0-9]/g, (d) => LEET[d] ?? d);
  if (BLOCKED.some((word) => squeezed.includes(word) || read.includes(word))) {
    return 'name not allowed';
  }
  const digits = name.replace(/[^0-9]/g, ' ').trim().split(/ +/);
  if (digits.some((group) => BLOCKED_CODES.includes(group))) return 'name not allowed';
  return null;
}
