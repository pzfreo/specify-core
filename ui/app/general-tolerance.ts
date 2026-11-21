// The general tolerance as the panel's row editor gives it (#228; specify-0.3.0.md, "A general
// tolerance given as a block", decision 2): either a class ("ISO 2768-m") or a block stated row by row
// (decimal places and their tolerance), with an angular line and any note lines. The editor composes
// the one line of text the CLI and edit lists take, so the text is the only contract with the engine:
//
//   .XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°      (inch: places written ".XX")
//   X ±0.5, X.X ±0.2, X.XX ±0.05, angles ±0°30′            (mm: places written "X.XX")
//
// The edit list's value for `part.general_tolerance` is that string, in `fill` or `change.to`.

export type Units = 'in' | 'mm';

/** One row of a block: so many decimal places, and their tolerance as written ("±.005"). */
export interface PlacesRow { places: number; tolerance: string }

export type GeneralTolerance =
  | { form: 'class'; value: string }
  | { form: 'block'; units: Units; rows: PlacesRow[]; angles: string; notes: string[] };

/** Decimal places as a title block writes them: inch ".XX", millimetre "X.XX" ("X" for none). */
export function placesText(places: number, units: Units): string {
  const xs = 'X'.repeat(Math.max(0, places));
  return units === 'in' ? `.${xs || 'X'}` : places > 0 ? `X.${xs}` : 'X';
}

/** The one line of text for ``gt``; empty lines and rows left blank are left out. */
export function compose(gt: GeneralTolerance): string {
  if (gt.form === 'class') return gt.value.trim();
  const parts = gt.rows.filter((r) => r.tolerance.trim()).map((r) => `${placesText(r.places, gt.units)} ${r.tolerance.trim()}`);
  parts.push(...gt.notes.map((n) => n.trim()).filter(Boolean));
  if (gt.angles.trim()) parts.push(`angles ${gt.angles.trim()}`);
  return parts.join(', ');
}

const PLACES = /^(X?)\.(X+)\s+(.+)$|^(X)\s+(.+)$/;

/** ``text`` read back into the editor: a block when it reads as one (a places row, or an angles
 * line), else a class. ``units`` is the file's length unit, used when the text does not say. */
export function parse(text: string, units: Units = 'mm'): GeneralTolerance {
  const items = text.split(',').map((s) => s.trim()).filter(Boolean);
  const rows: PlacesRow[] = [];
  const notes: string[] = [];
  let angles = '';
  let seen: Units | null = null;
  for (const item of items) {
    const m = PLACES.exec(item);
    if (m && m[2] !== undefined) {
      rows.push({ places: m[2].length, tolerance: m[3]!.trim() });
      seen ??= m[1] ? 'mm' : 'in';
    } else if (m && m[4] !== undefined) {
      rows.push({ places: 0, tolerance: m[5]!.trim() });
      seen ??= 'mm';
    } else if (/^angles?\b/i.test(item)) angles = item.replace(/^angles?\s*/i, '').trim();
    else notes.push(item);
  }
  if (!rows.length && !angles) return { form: 'class', value: text.trim() };
  return { form: 'block', units: seen ?? units, rows, angles, notes };
}

/** A new block, as the editor starts it in ``units``: two rows to fill in. */
export const emptyBlock = (units: Units): GeneralTolerance => ({
  form: 'block', units, rows: units === 'in' ? [{ places: 2, tolerance: '' }, { places: 3, tolerance: '' }]
    : [{ places: 0, tolerance: '' }, { places: 1, tolerance: '' }], angles: '', notes: [],
});
