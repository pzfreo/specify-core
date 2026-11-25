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
const ANYWHERE = new RegExp(String.raw`((?:[⌀Ø]|↧)\s?(?:${NUMBER}))(?![\d.]|\s?(?:mm|in)\b|\s?[±+]|\s?-\s?\d|/|–)`, 'g');
// Not lengths: a thread (its size is its name), roughness, a class, an angle.
const THREAD = /^(?:M\d|#?\d+(?:[-/]\d+)*\s?-\s?\d*\s?UN|Tr\d|G\d)/;
const NOT_LENGTH = /^(?:Ra|Rz|ISO|Sa)\b|°|^\d+× /;

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

/** A characteristic's text with its unit: what the file states, in its own; the rest in mm. */
export const labelRow = (row: { source: string; text: string; nominal: number | null; upper: number | null }): string => {
  if (!/^(?:general:)?existing:/.test(row.source)) return withUnits(row.text, 'mm');
  const unit = statedUnit(row.text.split(' from ')[0]!, row.nominal ?? row.upper);
  return unit ? withUnits(row.text, unit) : row.text;
};
