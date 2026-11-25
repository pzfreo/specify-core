// Lengths shown with their unit. Specify works in millimetres: geometry, suggestions and what is typed.
// What a file states is shown as it states it, in the file's unit, so in an inch file the two differ and
// each must say which it is. Only the text shown is changed, never a value.
export type LengthUnit = 'mm' | 'in';

const NUMBER = String.raw`\d+(?:\.\d+)?|\.\d+`;
// A length as a drawing reads it: ⌀0.438 ±0.005, 9.5 +0.1/-0.05, (0.75), 9.8–10.2, 0.005/0.25×0.25.
const LENGTH = String.raw`\(?(?:S?[⌀Ø□R]\s?|±\s?|[+-])?(?:${NUMBER})\)?`
  + String.raw`(?:\s?±\s?(?:${NUMBER})|\s?[+-]\s?(?:${NUMBER})\s?/\s?[+-]?\s?(?:${NUMBER})|/[+-]?(?:${NUMBER})(?:×(?:${NUMBER}))?|–(?:${NUMBER}))?`;
const CONTROL = '[⌖⌓⏥⟂∥∠⌒◎⌯○⌭↗⌰—⏤]';
// At the start of a piece, alone or after a control's symbol; and a diameter or a depth anywhere.
const FIRST = new RegExp(String.raw`^(${CONTROL}\s?)?(${LENGTH})(?=$|[\s,;)]|Ⓜ|Ⓛ)(?!\s?(?:mm|in|°|µm|×|x|-)(?:\W|$))`);
const ANYWHERE = new RegExp(String.raw`((?:[⌀Ø]|↧)\s?(?:${NUMBER})(?: × (?:${NUMBER}))?)(?![\d.]|\s?(?:mm|in)\b|\s?[±+]|\s?-\s?\d|/|–)`, 'g');
// Not lengths: a thread (its size is its name), roughness, a class, an angle.
const THREAD = /^(?:M\d|#?\d+(?:[-/]\d+)*\s?-\s?\d*\s?UN|Tr\d|G\d)/;
const NOT_LENGTH = /^(?:Ra|Rz|ISO|Sa)\b|°/;

/** ``text`` with ``unit`` after each length in it; pieces joined by " · " or "; " are read apart. */
export function withUnits(text: string, unit: LengthUnit): string {
  return text.split(/( · |; )/).map((piece, i) => {
    if (i % 2 || !/\d/.test(piece) || NOT_LENGTH.test(piece)) return piece;
    // A thread's depth: only where one unit serves the whole file (an inch file's is not known here).
    if (THREAD.test(piece)) return unit === 'mm' ? piece.replace(ANYWHERE, `$1 ${unit}`) : piece;
    const first = FIRST.exec(piece);
    if (!first) return piece.replace(ANYWHERE, `$1 ${unit}`);
    const rest = piece.slice(first[0].length).replace(ANYWHERE, `$1 ${unit}`);
    return `${first[0]} ${unit}${/^[ⓂⓁ]/.test(rest) ? ' ' : ''}${rest}`;
  }).join('');
}

/** The panel's line: the file's unit, and Specify's where it is another. */
export const unitLine = (file: LengthUnit): string => (file === 'in' ? 'File in inches · Specify works in mm' : 'Units: mm');

/** The unit a stated ``text`` reads in, from its value in mm (a range's, its middle); null if neither fits. */
export function statedUnit(text: string, mm: number | null | undefined): LengthUnit | null {
  if (!mm || text.includes('°')) return null;
  const range = new RegExp(String.raw`(${NUMBER})\)?\s?[–-]\s?[⌀ØR]?(${NUMBER})$`).exec(text);
  const first = new RegExp(NUMBER).exec(text);
  const shown = range ? (Number(range[1]) + Number(range[2])) / 2 : first ? Number(first[0]) : 0;
  const near = (value: number) => Math.abs(value - mm) <= 2e-3 * Math.abs(mm);
  return near(shown) ? 'mm' : near(shown * 25.4) ? 'in' : null;
}

/** ``text`` (stated items' texts, joined) with each one's own unit: what the file states is in the unit it
 * was written in, and Specify writes mm into an inch file too. One whose unit cannot be told says none. */
export function labelStated(text: string, existing: readonly { text?: string; value?: number | null }[]): string {
  return text.split(/( · |; )/).map((piece, i) => {
    if (i % 2) return piece;
    const units = new Set(existing.filter((e) => e.text === piece).map((e) => statedUnit(piece, e.value)));
    const [unit] = units;
    return units.size === 1 && unit ? withUnits(piece, unit) : piece;
  }).join('');
}

/** A dimension's text with its length in ``unit`` and what it is from ("from ⌀11.125 hole") in mm: a name is the
 * feature as Specify measures it, whatever the dimension was written in. */
function withOrigin(text: string, unit: LengthUnit): string {
  const at = text.indexOf(' from ');
  return at < 0 ? withUnits(text, unit) : `${withUnits(text.slice(0, at), unit)}${unit === 'in' ? withUnits(text.slice(at), 'mm') : text.slice(at)}`;
}

/** A characteristic's text with its unit: what the file states, in its own; the rest in mm. */
export const labelRow = (row: { source: string; text: string; nominal: number | null; upper: number | null }): string => {
  if (!/^(?:general:)?existing:/.test(row.source)) return withOrigin(row.text, 'mm');
  const unit = statedUnit(row.text.split(' from ')[0]!, row.nominal ?? row.upper);
  return unit ? withOrigin(row.text, unit) : row.text;
};

/** A feature's name or a question ("Ø50.8 hole"), which are in mm: said in an inch file, where the sizes beside
 * them are in inches; in a millimetre file everything is mm, and the panel says so. */
export const labelName = (name: string, file: LengthUnit): string => (
  // Only its diameters: "2 bottom faces" is a count.
  file === 'in' ? name.replace(ANYWHERE, '$1 mm') : name);

/** A row's numbers (nominal and deviations) are in: mm, or degrees, or µm for a roughness; nothing if it has none. */
export const numbersUnit = (row: { text: string; nominal: number | null; upper: number | null; unit?: string }): string => (
  row.nominal === null && row.upper === null ? ''
    : row.unit ?? (/°/.test(row.text) ? '°' : /\bR[az]\b/.test(row.text) ? 'µm' : 'mm'));

/** Float noise beyond ten significant digits dropped, as the engine shows a value (docs/answers.md). */
export const clean = (value: number): number => Number(value.toPrecision(10));

/** A row's text as an inspection file gives it: without noise, and with the unit its length is in, told from its
 * value (an inch file's own text is in inches, its numbers in mm); none where that cannot be told. */
export function exportText(row: { text: string; nominal: number | null; upper: number | null; unit?: string }): string {
  const text = row.text.replace(/\d+\.\d{8,}/g, (n) => `${clean(Number(n))}`);
  if (numbersUnit(row) !== 'mm') return text;
  const unit = statedUnit(text.split(' from ')[0]!, row.nominal ?? row.upper);
  // A control read by its kind's name, where the file gives no notation: "surface_profile 1.25".
  const named = /^([a-z]+(?:_[a-z]+)* )(.*)$/.exec(text);
  if (!unit) return text;
  return named ? `${named[1]}${withUnits(named[2]!, unit)}` : withOrigin(text, unit);
}

/** The unit the features' names need saying in, for ``rows``: an inch file's stated rows read in inches beside them. */
export const rowsUnit = (rows: readonly { text: string; nominal: number | null; upper: number | null }[]): LengthUnit => (
  rows.some((r) => statedUnit(r.text.split(' from ')[0]!, r.nominal ?? r.upper) === 'in') ? 'in' : 'mm');
