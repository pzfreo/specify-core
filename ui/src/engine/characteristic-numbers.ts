import type { PmiIntent, PmiIntentItem } from '../types/pmi';

/**
 * The dimension planner's id without how the requirement came to be (``specified:``,
 * ``answered:``, ``stored:…:{nominal}:{tolerance}``), as pmiScene names it (ADR P11, D3).
 */
export function plannerId(id: string): string {
  for (const prefix of ['specified:', 'answered:']) if (id.startsWith(prefix)) return id.slice(prefix.length);
  if (!id.startsWith('stored:')) return id;
  let rest = id.slice('stored:'.length);
  // The tolerance is a JSON value: null, or a string with no quote inside it.
  if (rest.endsWith(':null')) rest = rest.slice(0, -':null'.length);
  else if (rest.endsWith('"') && rest.lastIndexOf(':"') >= 0) rest = rest.slice(0, rest.lastIndexOf(':"'));
  const nominal = rest.lastIndexOf(':');
  return nominal >= 0 ? rest.slice(0, nominal) : rest;
}

/** The IDs in specify-pmi 0.2.0's pmiScene identify semantic requirements independently of
 * values. Their number metadata comes from the same allocation used by the STEP writer. */
function sceneId(item: PmiIntentItem): string {
  const planner = item['planner'] as { required?: { id?: string } } | undefined;
  if (planner?.required?.id) return `planner:${plannerId(planner.required.id)}`;
  if (typeof item['letter'] === 'string') return `datum:${item['letter']}`;
  const question = item.question ?? '';
  const feature = question.startsWith('extra.') ? question : item['feature'];
  const target = typeof feature === 'string' && feature ? feature : `faces:${JSON.stringify(item.faces)}`;
  const base = `${item.kind ?? 'item'}:${target}`;
  return item.kind === 'location'
    ? `${base}:${JSON.stringify(item['reference'] ?? null)}${question ? `:${question}` : ''}`
    : base;
}

export interface NumberScene {
  specify: Record<string, { number?: number }>;
}

/** Preserve stored numbers and allocate new ones by asking the engine, never by row order. */
export function withCharacteristicNumbers(intent: PmiIntent, scene: NumberScene): PmiIntent {
  const numbered = <T extends PmiIntentItem>(item: T): T => ({
    ...item, number: scene.specify[sceneId(item)]?.number,
  });
  const partNumbers = Object.fromEntries(Object.keys(intent.part).flatMap((key) => {
    const number = scene.specify[`part.${key}`]?.number;
    return number === undefined ? [] : [[key, number]];
  }));
  return {
    ...intent, datums: intent.datums.map(numbered), requirements: intent.requirements.map(numbered),
    general: intent.general?.map(numbered), partNumbers,
  };
}
