// The general tolerance as the panel's row editor gives it (#228; specify-0.3.0.md, "A general
// tolerance given as a block", decision 2): either a class ("ISO 2768-m") or a block stated row by row
// (decimal places and their tolerance), with an angular line and any note lines. The editor composes
// the one line of text the CLI and edit lists take, so the text is the only contract with the engine:
//
//   .XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°      (inch: places written ".XX")
//   X ±0.5 mm, X.X ±0.2, X.XX ±0.05, angles ±0°30′         (mm: places written "X.XX"; no places "X", with its unit)
//
// The edit list's value for `part.general_tolerance` is that string, in `fill` or `change.to`.

export type Units = 'in' | 'mm';

/** One row of a block: so many decimal places, and their tolerance as written ("±.005"). */
export interface PlacesRow { places: number; tolerance: string }

export type GeneralTolerance =
  | { form: 'class'; value: string }
  | { form: 'block'; units: Units; rows: PlacesRow[]; angles: string; notes: string[] };

/** Decimal places as a title block writes them: inch ".XX", millimetre "X.XX", and "X" for none in
 * either (the engine's spelling, src/tolerance_block.rs; a line of no places states its unit). */
export function placesText(places: number, units: Units): string {
  const xs = 'X'.repeat(Math.max(0, places));
  if (places <= 0) return 'X';
  return units === 'in' ? `.${xs}` : `X.${xs}`;
}

/** The one line of text for ``gt``; empty lines and rows left blank are left out. The unit is
 * written after a row's tolerance (`X ±.5 in`) where the notation does not say it: on every row of
 * no places, and on the first row when the block is not in ``fileUnits`` and no `.XX` row says
 * inches. A block of angles or notes alone has no length to give a unit to. */
export function compose(gt: GeneralTolerance, fileUnits: Units = 'mm'): string {
  if (gt.form === 'class') return gt.value.trim();
  const rows = gt.rows.filter((r) => r.tolerance.trim());
  const implied = gt.units === 'in' ? rows.some((r) => r.places > 0) : gt.units === fileUnits;
  const parts = rows.map((r, i) => {
    const unit = r.places <= 0 || (i === 0 && !implied) ? ` ${gt.units}` : '';
    return `${placesText(r.places, gt.units)} ${r.tolerance.trim()}${unit}`;
  });
  parts.push(...gt.notes.map((n) => n.trim()).filter(Boolean));
  if (gt.angles.trim()) parts.push(`angles ${gt.angles.trim()}`);
  return parts.join(', ');
}

const PLACES = /^(X?)\.(X+)\s+(\S+)(?:\s+(in|inch|inches|"|mm))?$|^X\s+(\S+)(?:\s+(in|inch|inches|"|mm))?$/i;
const unitOf = (text: string): Units => (text.toLowerCase() === 'mm' ? 'mm' : 'in');

/** ``text`` read back into the editor: a block when it reads as one (a places row, or an angles
 * line), else a class. Lines are separated by `,`, `;`, `·` or new lines, as the engine reads them.
 * The unit: one written on a row, else inches for `.XX` rows, else ``units`` (the file's). */
export function parse(text: string, units: Units = 'mm'): GeneralTolerance {
  const items = text.split(/[,;·\n]/).map((s) => s.trim()).filter(Boolean);
  const rows: PlacesRow[] = [];
  const notes: string[] = [];
  let angles = '';
  let stated: Units | null = null;
  let inchStyle = false;
  for (const item of items) {
    const m = PLACES.exec(item.replace(/\s+/g, ' '));
    if (m && m[2] !== undefined) {
      rows.push({ places: m[2].length, tolerance: m[3]! });
      if (!m[1]) inchStyle = true;
      if (m[4]) stated ??= unitOf(m[4]);
    } else if (m && m[5] !== undefined) {
      rows.push({ places: 0, tolerance: m[5] });
      if (m[6]) stated ??= unitOf(m[6]);
    } else if (/^(angles?|angular)\b/i.test(item)) angles = item.replace(/^(angles?|angular)\s*:?\s*/i, '').trim();
    else notes.push(item);
  }
  if (!rows.length && !angles) return { form: 'class', value: text.trim() };
  return { form: 'block', units: stated ?? (inchStyle ? 'in' : units), rows, angles, notes };
}

/** A new block, as the editor starts it in ``units``: two rows to fill in. */
export const emptyBlock = (units: Units): GeneralTolerance => ({
  form: 'block', units, rows: units === 'in' ? [{ places: 2, tolerance: '' }, { places: 3, tolerance: '' }]
    : [{ places: 0, tolerance: '' }, { places: 1, tolerance: '' }], angles: '', notes: [],
});

/** The file's length unit, as its STEP declares it (an inch file names `CONVERSION_BASED_UNIT('INCH', …)`
 * for its lengths); millimetres otherwise. Read from the bytes until the analysis carries the unit the
 * engine reads (`declared_length_unit`). */
export function fileLengthUnit(bytes: Uint8Array): 'in' | 'mm' {
  const text = new TextDecoder('latin1').decode(bytes);
  return /CONVERSION_BASED_UNIT\s*\(\s*'INCH(ES)?'/i.test(text) ? 'in' : 'mm';
}
