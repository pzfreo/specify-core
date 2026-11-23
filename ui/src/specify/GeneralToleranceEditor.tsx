import { useEffect, useId, useState } from 'react';
import { compose, emptyBlock, parse, placesText, type GeneralTolerance, type Units } from './general-tolerance';

/** The general tolerance's editor (#228 decision 2): a class, or a block row by row (decimal places and
 * their tolerance, Add row), an angular line and note lines. It edits the one line of text it composes. */
export function GeneralToleranceEditor({ text, units, classes, onChange, onClass }: {
  text: string; units: Units; classes: readonly string[];
  onChange(text: string, form: GeneralTolerance['form']): void;
  onClass?: (value: string) => void;
}) {
  const editorId = useId();
  const [gt, setGt] = useState<GeneralTolerance>(() => (text.trim() ? parse(text, units) : { form: 'class', value: classes[0] ?? '' }));
  const set = (next: GeneralTolerance) => { setGt(next); onChange(compose(next, units), next.form); };
  useEffect(() => { onChange(compose(gt, units), gt.form); }, []);
  const block = gt.form === 'block' ? gt : null;
  const notes = block?.notes.length ? block.notes : [''];
  return (
    <div className="spec-gt" role="group" aria-label="General tolerance">
      <div className="spec-gt-form">
        <label><input type="radio" name={editorId} checked={gt.form === 'class'} onChange={() => set({ form: 'class', value: classes[1] ?? classes[0] ?? '' })} /> A class</label>
        <label><input type="radio" name={editorId} checked={gt.form === 'block'} onChange={() => set(emptyBlock(units))} /> A block, row by row</label>
      </div>
      {gt.form === 'class' && (onClass ? (
        <div className="opts">
          {classes.map(c => <button key={c} type="button" onClick={() => onClass(c)}>{c}</button>)}
        </div>
      ) : (
        <select aria-label="General tolerance class" value={gt.value} onChange={(e) => set({ form: 'class', value: e.target.value })}>
          {!classes.includes(gt.value) && <option value={gt.value}>{gt.value}</option>}
          {classes.map((c) => <option key={c} value={c}>{c}</option>)}
        </select>
      ))}
      {block && (<>
        <label className="spec-gt-units">Units{' '}
          <select aria-label="Units" value={block.units} onChange={(e) => set({ ...block, units: e.target.value as Units })}>
            <option value="mm">mm</option><option value="in">in</option>
          </select>
        </label>
        {block.rows.map((r, i) => (
          <div key={i} className="spec-gt-row" data-gt-row={i}>
            <select aria-label={`Row ${i + 1} decimal places`} value={r.places}
              onChange={(e) => set({ ...block, rows: block.rows.map((x, j) => (j === i ? { ...x, places: Number(e.target.value) } : x)) })}>
              {[0, 1, 2, 3, 4].map((n) => <option key={n} value={n}>{placesText(n, block.units)}</option>)}
            </select>
            <input aria-label={`Row ${i + 1} tolerance`} placeholder={block.units === 'in' ? '±.005' : '±0.1'} value={r.tolerance}
              onChange={(e) => set({ ...block, rows: block.rows.map((x, j) => (j === i ? { ...x, tolerance: e.target.value } : x)) })} />
            <button type="button" aria-label={`Remove row ${i + 1}`} onClick={() => set({ ...block, rows: block.rows.filter((_, j) => j !== i) })}>✕</button>
          </div>
        ))}
        <button type="button" className="spec-gt-add" onClick={() => {
          const last = block.rows.at(-1)?.places ?? -1;
          set({ ...block, rows: [...block.rows, { places: Math.min(4, last + 1), tolerance: '' }] });
        }}>Add row</button>
        <label className="spec-gt-line">Angles <input aria-label="Angles" placeholder="±1°" value={block.angles} onChange={(e) => set({ ...block, angles: e.target.value })} /></label>
        {notes.map((note, i) => (
          <div key={i} className="spec-gt-line">
            <label htmlFor={`${editorId}-note-${i}`}>Note {i + 1}</label>
            <input id={`${editorId}-note-${i}`} aria-label={`Note ${i + 1}`} placeholder="fractions ±1/64" value={note}
              onChange={(e) => set({ ...block, notes: notes.map((n, j) => j === i ? e.target.value : n) })} />
            <button type="button" aria-label={`Remove note ${i + 1}`}
              onClick={() => set({ ...block, notes: notes.filter((_, j) => j !== i) })}>✕</button>
          </div>
        ))}
        <button type="button" className="spec-gt-add" onClick={() => set({ ...block, notes: [...notes, ''] })}>Add note</button>
        <div className="muted spec-gt-text">Written as: <code>{compose(block, units) || '…'}</code></div>
      </>)}
    </div>
  );
}
