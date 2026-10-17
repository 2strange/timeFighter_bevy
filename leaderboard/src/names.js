// Pilot names: 1 to 18 printable characters of any script, as the game
// takes them (src/score/board.rs), plus a short list of words that do not
// go on a public list. Names are only ever shown as text (textContent in
// the page, plain text in the game), never as HTML.

export const NAME_MAX = 18;

// Control and invisible formatting characters (Cc, Cf), and the
// replacement character a broken encoding leaves.
const INVISIBLE = /[\p{Cc}\p{Cf}�]/gu;

// Checked against the name folded to A-Z and 0-9 (accents dropped, digits
// read as the letters they stand in for).
const BLOCKED = [
  'NAZI',
  'HITLER',
  'HEILH',
  'SIEGHEIL',
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
// Number codes of the far right, checked as whole digit groups.
const BLOCKED_CODES = ['88', '1488'];
const LEET = { 0: 'O', 1: 'I', 3: 'E', 4: 'A', 5: 'S', 7: 'T', 8: 'B' };

/** The name as it is stored: invisible characters out, trimmed, NFC. */
export function cleanName(name) {
  if (typeof name !== 'string') return '';
  return name.normalize('NFC').replace(INVISIBLE, '').trim();
}

/** A reason why `name` (already cleaned) cannot go on the list, or null. */
export function checkName(name) {
  if (typeof name !== 'string' || name.length === 0) return 'no name';
  const length = Array.from(name).length;
  if (length > NAME_MAX) return `name must have 1 to ${NAME_MAX} characters`;
  const folded = name
    .normalize('NFKD')
    .replace(/\p{M}/gu, '')
    .toUpperCase()
    .replace(/[^A-Z0-9]/g, '');
  const read = folded.replace(/[0-9]/g, (d) => LEET[d] ?? d);
  if (BLOCKED.some((word) => folded.includes(word) || read.includes(word))) {
    return 'name not allowed';
  }
  const digits = name.match(/[0-9]+/g) ?? [];
  if (digits.some((group) => BLOCKED_CODES.includes(group))) return 'name not allowed';
  return null;
}
