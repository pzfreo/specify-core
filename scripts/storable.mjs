// ADR P11 rule 2, as src/feature_table.rs `storable`: a file stores only the answers that leave
// no PMI behind (a choice that is not a requirement, one decided none, or an approval the planner
// wrote nothing for, listed in `_specify.no_pmi`); the file's PMI is the only copy of the rest. The node scripts compare what a reopened file resumes with this.
const NOT_REQUIREMENTS = ['part.locating'];

function isRequirement(id) {
  return !(id.startsWith('_') || id.startsWith('bom.') || NOT_REQUIREMENTS.includes(id));
}

function isNone(value) {
  if (value === null || value === undefined) return true;
  if (typeof value === 'string') {
    const text = value.trim();
    return text === '' || text.toLowerCase().startsWith('none');
  }
  return Array.isArray(value) && value.length === 0;
}

export function storable(answers) {
  const nothing = answers._specify?.no_pmi ?? [];
  return Object.fromEntries(Object.entries(answers)
    .filter(([id, value]) => !isRequirement(id) || isNone(value) || nothing.includes(id)));
}
