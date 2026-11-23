const ICON = {
  edit: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M11 2.5l2.5 2.5L6 12.5l-3 .5.5-3z" /></svg>,
  bin: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 8.5h6l.5-8.5M6.8 7v4M9.2 7v4" /></svg>,
};

/** An icon button: a pencil or a bin, the same size and colour (decision 1). */
export const IconButton = ({ label, icon, onClick, disabled }: { label: string; icon: keyof typeof ICON; onClick: () => void; disabled?: boolean }) => (
  <button type="button" className="spec-icon" title={label} aria-label={label} onClick={onClick} disabled={disabled}>{ICON[icon]}</button>
);

/** The one-line remove confirmation: the only red in the panel (decision 1). */
export function RemoveLine({ what, numbers, checking, problem, onRemove, onCancel }: {
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
