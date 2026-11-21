// The review panel (specify-panel.md, accepted 2026-11-20, with its mock-up): the part's features,
// each kind of requirement it can need shown stated, a gap, or decided none. Stated rows have two
// matching, uncoloured icons, change and remove; a gap shows its suggestion and offers Add and "None
// needed"; a decided none offers Undo. Every edit is held in the page (the bar at the bottom lists
// them) until Save writes them together. Who wrote a requirement is never shown: only the page's
// unsaved edits are highlighted.
import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { EditFailure, EditList } from './local.ts';
import { describe, removalTargets, type Pending, type PendingEdits } from './edits.ts';
import { answerText, balloonText, changedText, isNoneValue, type OtherRow, type PanelGroup, type PanelModel, type PanelRow } from './table.ts';
import { compose, emptyBlock, parse, placesText, type GeneralTolerance, type Units } from './general-tolerance.ts';

export { describe };

const GENERAL_TOLERANCE = 'part.general_tolerance';

const ICON = {
  edit: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M11 2.5l2.5 2.5L6 12.5l-3 .5.5-3z" /></svg>,
  bin: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 8.5h6l.5-8.5M6.8 7v4M9.2 7v4" /></svg>,
};

/** An icon button: a pencil or a bin, the same size and colour (decision 1). */
const IconButton = ({ label, icon, onClick, disabled }: { label: string; icon: keyof typeof ICON; onClick: () => void; disabled?: boolean }) => (
  <button type="button" className="spec-icon" title={label} aria-label={label} onClick={onClick} disabled={disabled}>{ICON[icon]}</button>
);

/** What the person types, as the answer the edit list sends: faces as numbers, a list one per line. */
function valueOf(row: PanelRow, text: string): unknown {
  const kind = row.question?.kind as string | undefined;
  if (kind === 'faces') return text.split(/[\s,;]+/).filter(Boolean).map(Number);
  if (kind === 'text_list') return text.split('\n').map((s) => s.trim()).filter(Boolean);
  return text.trim();
}

/** An answer as the person edits it: faces "0, 1", a list one per line, else as is. */
function inputOf(row: PanelRow, value: unknown): string {
  if (value === null || value === undefined) return '';
  if (Array.isArray(value)) return value.join((row.question?.kind as string | undefined) === 'text_list' ? '\n' : ', ');
  return typeof value === 'string' ? value : JSON.stringify(value);
}

/** A change starts from the file's value when it is one Specify can take back as an answer; for a
 * tolerance written with its frame ("Position 0.25 | A") from its magnitude. */
function changeStart(row: PanelRow): string {
  const q = row.question;
  const now = typeof row.expect === 'string' ? row.expect : inputOf(row, row.expect);
  if (!q || q.kind === 'text' || q.options?.includes(now)) return now;
  if (q.options?.length && q.options.every((o) => /^(none|[\d.]+)$/.test(o))) return /\d+(?:\.\d+)?/.exec(now)?.[0] ?? '';
  return q.kind === 'faces' ? '' : now;
}

interface Editing { id: string; mode: 'add' | 'change' | 'remove' }

export interface PanelProps {
  filename: string;
  model: PanelModel;
  pending: PendingEdits;
  onPending(next: PendingEdits): void;
  /** Checks an edit list against the file (all or nothing, as Save would): the entries that cannot be applied. */
  check(edits: EditList): Promise<EditFailure[]>;
  /** Values the session holds that the file does not yet (an agent's answers, not saved): shown as unsaved. */
  unsaved: Readonly<Record<string, unknown>>;
  /** Rows can't be changed here (a part of an assembly, on this base): why, shown once. */
  readOnly: string | null;
  disabled: boolean;
  /** The file's length unit, for a new general tolerance block. */
  units: Units;
  /** The characteristics table, rendered by the review into this element. */
  characteristics: { element: HTMLElement; count: number } | null;
  /** Light ``faces`` on the part. */
  onFaces(faces: readonly number[]): void;
  /** Turn to row ``id`` (a decision picked in the diff); a new ``n`` each time. */
  focus: { id: string; n: number } | null;
}

export function Panel(props: PanelProps) {
  const { model, pending, onPending, readOnly, disabled } = props;
  const [tab, setTab] = useState<'review' | 'table'>('review');
  const [editing, setEditing] = useState<Editing | null>(null);
  const [problem, setProblem] = useState<{ id: string; text: string } | null>(null);
  const [checking, setChecking] = useState(false);
  const body = useRef<HTMLDivElement>(null);
  const chars = useRef<HTMLDivElement>(null);

  // The characteristics table is the review's (read only), shown under its tab.
  useEffect(() => {
    const el = props.characteristics?.element;
    if (el && chars.current && el.parentElement !== chars.current) chars.current.appendChild(el);
  }, [props.characteristics?.element, tab]);
  // A focus request turns to its row.
  useEffect(() => {
    if (!props.focus) return;
    setTab('review');
    requestAnimationFrame(() => body.current?.querySelector(`[data-row="${CSS.escape(props.focus!.id)}"]`)?.scrollIntoView({ block: 'center' }));
  }, [props.focus?.n]);
  // Discard (or a save) closes any line that was open on an edit now gone.
  useEffect(() => { if (!pending.size) { setEditing(null); setProblem(null); } }, [pending]);

  const close = () => { setEditing(null); setProblem(null); };
  /** ``p`` joins the bar once the file takes the whole list with it (every edit already listed, as
   * Save would send them), so the bar always holds a list Save can write. Any failure, on this edit or
   * on one listed earlier that it breaks, is said on its line, and it does not join. */
  const apply = async (p: Pending) => {
    setChecking(true);
    try {
      const next = pending.with(p);
      const failures = await props.check(next.list());
      if (failures.length) { setProblem({ id: p.id, text: failures.map((f) => f.reason).join('; ') }); return; }
      onPending(next);
      close();
    } finally { setChecking(false); }
  };

  const open = (id: string, mode: Editing['mode']) => { setProblem(null); setEditing({ id, mode }); };

  const row = (r: PanelRow) => {
    const p = pending.get(r.id);
    const agent = r.id in props.unsaved ? props.unsaved[r.id] : undefined;
    const shown = p?.kind === 'fill' || p?.kind === 'change' ? p.text
      : p?.kind === 'none' ? 'none needed' : p?.kind === 'clear' ? 'not stated'
        : agent !== undefined ? (isNoneValue(agent) ? 'none needed' : answerText(agent, r.question)) : r.value;
    // What the row is now, with the page's edit laid over the file.
    const state = p?.kind === 'fill' || p?.kind === 'change' ? 'stated' : p?.kind === 'none' ? 'decided_none'
      : p?.kind === 'clear' ? 'gap' : agent !== undefined ? (isNoneValue(agent) ? 'decided_none' : 'stated') : r.state;
    const cls = ['spec-row', state === 'stated' ? 'stated' : state === 'gap' ? 'gap' : 'nonerow',
      p || agent !== undefined ? 'changed' : '', p?.kind === 'remove' ? 'removed' : ''].filter(Boolean).join(' ');
    const here = editing?.id === r.id ? editing : null;
    const locked = Boolean(readOnly) || disabled;
    let acts: ReactNode = null;
    if (!readOnly && p?.kind !== 'remove') {
      if (state === 'stated') {
        acts = (
          <span className="acts">
            <IconButton label={`Change ${r.name}`} icon="edit" disabled={locked} onClick={() => open(r.id, p?.kind === 'fill' ? 'add' : 'change')} />
            {/* Every stated row has both (decisions 1 and 2); on an Add not yet saved the bin takes it back. */}
            <IconButton label={`Remove ${r.name}`} icon="bin" disabled={locked}
              onClick={() => (p?.kind === 'fill' ? onPending(pending.without(r.id)) : open(r.id, 'remove'))} />
          </span>
        );
      } else if (state === 'gap') {
        acts = (
          <span className="acts">
            <button type="button" disabled={locked} onClick={() => open(r.id, 'add')}>Add</button>
            <button type="button" disabled={locked} onClick={() => (p?.kind === 'clear' ? onPending(pending.without(r.id))
              : void apply({ kind: 'none', id: r.id, name: r.name }))}>None needed</button>
          </span>
        );
      } else {
        acts = (
          <span className="acts">
            <button type="button" disabled={locked} onClick={() => (p?.kind === 'none' ? onPending(pending.without(r.id))
              : void apply({ kind: 'clear', id: r.id, name: r.name }))}>Undo</button>
          </span>
        );
      }
    }
    return (
      <div key={r.id} className={cls} data-row={r.id} data-question={r.id} data-state={state}>
        <button type="button" className="k" title="Show it on the part" onClick={() => props.onFaces(r.faces)}>{r.label}</button>
        <span className="v">
          <span className="t">{shown}</span>
          {state === 'gap' && r.suggestionText !== undefined && <span className="sugg">suggested: {r.suggestionText}</span>}
        </span>
        {acts}
        {here && here.mode !== 'remove' && (
          <EditLine row={r} mode={here.mode} pending={p} units={props.units} checking={checking} problem={problem?.id === r.id ? problem.text : null}
            onCancel={close} onApply={(value, text) => {
              if (here.mode === 'change') {
                const was = r.value;
                if (text === was || (p?.kind !== 'change' && JSON.stringify(value) === JSON.stringify(r.expect))) { onPending(pending.without(r.id)); close(); return; }
                void apply({ kind: 'change', id: r.id, name: r.name, value, text: r.id === GENERAL_TOLERANCE ? text : changedText(was, value, r.question), expect: r.expect, was });
              } else void apply({ kind: 'fill', id: r.id, name: r.name, value, text });
            }} />
        )}
        {here?.mode === 'remove' && (
          <RemoveLine what={shown} numbers={r.numbers} problem={problem?.id === r.id ? problem.text : null} checking={checking} onCancel={close}
            onRemove={() => void apply({ kind: 'remove', id: r.id, name: r.name, targets: removalTargets(r.numbers, r.id), was: r.value })} />
        )}
      </div>
    );
  };

  const other = (o: OtherRow) => {
    const p = pending.get(o.key);
    const here = editing?.id === o.key;
    return (
      <div key={o.key} className={`spec-row stated${p ? ' changed removed' : ''}`} data-row={o.key}>
        <button type="button" className="k" title="Show it on the part" onClick={() => props.onFaces(o.faces)}>{o.label}</button>
        <span className="v">{o.value}{o.number && <span className="num">{o.number}</span>}</span>
        {!readOnly && !p && (o.number || o.id) && (
          <span className="acts"><IconButton label={`Remove ${o.value}`} icon="bin" disabled={disabled} onClick={() => open(o.key, 'remove')} /></span>
        )}
        {here && (
          <RemoveLine what={o.value} numbers={o.number ? [o.number] : []} problem={problem?.id === o.key ? problem.text : null} checking={checking} onCancel={close}
            onRemove={() => void apply({ kind: 'remove', id: o.key, name: o.label, targets: removalTargets(o.number ? [o.number] : [], o.id), was: o.value })} />
        )}
      </div>
    );
  };

  const group = (g: PanelGroup | null, heading?: string) => g && g.rows.length > 0 && (
    <div className="spec-feat" key={g.key} data-group={g.key}>
      {heading === undefined ? null : <div className="name">{heading}{balloon(g)}</div>}
      {g.rows.map(row)}
    </div>
  );
  const balloon = (g: PanelGroup) => {
    const text = balloonText(g.rows.filter((r) => r.state === 'stated').flatMap((r) => r.numbers));
    return text ? <span className="balloon">{text}</span> : null;
  };

  return (
    <div className="pmi-lab pmi-lab-pane">
      <aside className="pmi-lab-panel spec-panel">
        <header>
          <div className="title"><strong>{props.filename}</strong></div>
          {readOnly && <p className="pmi-lab-note spec-readonly" role="status">{readOnly}</p>}
          <div className="pmi-lab-views" role="tablist">
            <button type="button" role="tab" aria-selected={tab === 'review'} className={tab === 'review' ? 'on' : ''} onClick={() => setTab('review')}>
              {readOnly ? 'Review' : 'Review / Edit'}
            </button>
            <button type="button" role="tab" aria-selected={tab === 'table'} className={tab === 'table' ? 'on' : ''} onClick={() => setTab('table')}>
              Characteristics ({props.characteristics?.count ?? 0})
            </button>
          </div>
        </header>
        <div className="spec-chars" ref={chars} hidden={tab !== 'table'} />
        <div className="spec-rows" ref={body} hidden={tab !== 'review'}>
          {model.part && model.part.rows.length > 0 && (<>
            <div className="spec-group">The part</div>
            {group(model.part)}
          </>)}
          {model.bom && (<><div className="spec-group">Parts list</div>{group(model.bom)}</>)}
          {model.features.length > 0 && <div className="spec-group">On the part</div>}
          {model.features.map((g) => group(g, g.title))}
          {model.datums && (<><div className="spec-group">Datums</div>{group(model.datums)}</>)}
          {model.other.length > 0 && (<>
            <div className="spec-group">Other PMI in the file</div>
            <div className="spec-feat" data-group="other">{model.other.map(other)}</div>
          </>)}
        </div>
      </aside>
    </div>
  );
}

/** The one line under a row for Add or Change (decision 2: the current value first, no dialog). */
function EditLine({ row, mode, pending, units, checking, problem, onApply, onCancel }: {
  row: PanelRow; mode: 'add' | 'change'; pending: Pending | undefined; units: Units; checking: boolean; problem: string | null;
  onApply(value: unknown, text: string): void; onCancel(): void;
}) {
  const start = pending?.kind === 'fill' || pending?.kind === 'change' ? inputOf(row, pending.value)
    : mode === 'add' ? inputOf(row, row.suggestion) : changeStart(row);
  const [text, setText] = useState(start);
  const gt = row.id === GENERAL_TOLERANCE;
  const list = (row.question?.kind as string | undefined) === 'text_list';
  const number = row.numbers.length === 1 ? row.numbers[0] : row.numbers.length ? row.numbers.join(', ') : '';
  const submit = () => {
    const value = valueOf(row, text);
    if (isNoneValue(value) && !(value === 'none' || (Array.isArray(value) && mode === 'add'))) return;
    onApply(value, gt ? text.trim() : answerText(value, row.question));
  };
  const options = `spec-options-${row.id.replace(/[^\w-]/g, '_')}`;
  return (
    <div className="spec-edit">
      <div className="now">{mode === 'change' ? <>Now: <b>{row.value}</b></> : 'Nothing stated yet.'}</div>
      {gt ? <GeneralToleranceEditor text={text} units={units} classes={row.question?.options ?? []} onChange={setText} />
        : list ? <textarea aria-label="Value" value={text} rows={3} onChange={(e) => setText(e.target.value)} />
          : (<>
            <input aria-label={mode === 'change' ? 'New value' : 'Value'} value={text} list={row.question?.options?.length ? options : undefined}
              placeholder={row.question?.kind === 'faces' ? 'face numbers, e.g. 0, 4' : row.question?.options?.[0] ?? ''}
              onChange={(e) => setText(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') submit(); if (e.key === 'Escape') onCancel(); }} autoFocus />
            {row.question?.options?.length ? (
              <datalist id={options}>{row.question.options.map((o) => <option key={o} value={o} label={answerText(o, row.question)} />)}</datalist>
            ) : null}
          </>)}
      {problem && <div className="spec-problem" role="alert">{problem}</div>}
      <div className="foot">
        <button type="button" className="primary" disabled={checking} onClick={submit}>{mode === 'change' ? 'Apply' : 'Add'}</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
      {mode === 'change' && number && (
        <div className="muted">Keeps {number} if only the limits change. A different measurement gets a new number and {number} is retired.</div>
      )}
    </div>
  );
}

/** The one-line remove confirmation: the only red in the panel (decision 1). */
function RemoveLine({ what, numbers, checking, problem, onRemove, onCancel }: {
  what: string; numbers: readonly string[]; checking: boolean; problem: string | null; onRemove(): void; onCancel(): void;
}) {
  const n = numbers.join(', ');
  return (
    <div className="spec-edit rm">
      <div className="now">Remove <b>{what}</b>?{n ? ` Number ${n} ${numbers.length > 1 ? 'are' : 'is'} retired and never reused.` : ''}</div>
      {problem && <div className="spec-problem" role="alert">{problem}</div>}
      <div className="foot">
        <button type="button" className="danger" disabled={checking} onClick={onRemove}>Remove</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}

/** The general tolerance's editor (#228 decision 2): a class, or a block row by row (decimal places and
 * their tolerance, Add row), an angular line and note lines. It edits the one line of text it composes. */
export function GeneralToleranceEditor({ text, units, classes, onChange }: {
  text: string; units: Units; classes: readonly string[]; onChange(text: string): void;
}) {
  const [gt, setGt] = useState<GeneralTolerance>(() => (text.trim() ? parse(text, units) : { form: 'class', value: classes[0] ?? '' }));
  const set = (next: GeneralTolerance) => { setGt(next); onChange(compose(next, units)); };
  useEffect(() => { onChange(compose(gt, units)); }, []);
  const block = gt.form === 'block' ? gt : null;
  return (
    <div className="spec-gt" role="group" aria-label="General tolerance">
      <div className="spec-gt-form">
        <label><input type="radio" name="gt-form" checked={gt.form === 'class'} onChange={() => set({ form: 'class', value: classes[1] ?? classes[0] ?? '' })} /> A class</label>
        <label><input type="radio" name="gt-form" checked={gt.form === 'block'} onChange={() => set(emptyBlock(units))} /> A block, row by row</label>
      </div>
      {gt.form === 'class' && (
        <select aria-label="General tolerance class" value={gt.value} onChange={(e) => set({ form: 'class', value: e.target.value })}>
          {!classes.includes(gt.value) && <option value={gt.value}>{gt.value}</option>}
          {classes.map((c) => <option key={c} value={c}>{c}</option>)}
        </select>
      )}
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
        <label className="spec-gt-line">Note <input aria-label="Note" placeholder="fractions ±1/64" value={block.notes.join(', ')}
          onChange={(e) => set({ ...block, notes: e.target.value ? [e.target.value] : [] })} /></label>
        <div className="muted spec-gt-text">Written as: <code>{compose(block, units) || '…'}</code></div>
      </>)}
    </div>
  );
}

/** The bar at the bottom (decision 3): the pending edits, Save and Discard. */
export function EditBar({ pending, saving, blocked, savedNote, onSave, onDiscard, onAddAll, adding, readOnly }: {
  pending: PendingEdits; saving: boolean; blocked: string | undefined; savedNote: string | null;
  onSave(): void; onDiscard(): void; onAddAll(): void; adding: boolean; readOnly: string | null;
}) {
  const items = pending.values();
  return (
    <div className="spec-bar" role="region" aria-label="Changes to save">
      <ul>
        {items.length ? items.map((p) => <li key={p.id} data-pending={p.id}>{describe(p)}</li>)
          : <li className="muted">{savedNote ?? (readOnly ? readOnly : 'No changes yet. Nothing is written until Save.')}</li>}
      </ul>
      <button type="button" className="app-add-all" onClick={onAddAll} disabled={saving || adding || Boolean(readOnly) || Boolean(blocked)}
        title="Queue every gap's suggestion as an edit (a suggestion of none as None needed); nothing is written until Save">
        Add all suggestions
      </button>
      <button type="button" className="app-discard" onClick={onDiscard} disabled={!items.length || saving}>Discard</button>
      <button type="button" className="app-save" onClick={onSave} disabled={saving || Boolean(blocked)} title={blocked}>
        {saving ? 'Saving…' : 'Save'}
      </button>
    </div>
  );
}
