//! The edit list (proposal specify-0.3.0, use case 7 and the Appendix; ADR P11): what a caller
//! asks `write`, MCP, the session or Python to do to a file, and its all-or-nothing validation
//! against the file's state.
//!
//! ```json
//! {
//!   "fill":   {"hole.position:holes:12": "0.2"},
//!   "change": {"hole.position:holes:14": {"to": "0.1", "expect": "0.2"}},
//!   "items":  {"change": {"#21": {"tolerance": "±0.05", "expect": "±0.1"}},
//!              "remove": ["#17"]},
//!   "none":   ["part.coating", "datum.A.flatness"],
//!   "accept_suggestions": ["hole.size:holes:3"],
//!   "clear":  ["part.locating"]
//! }
//! ```
//!
//! Ids are feature-and-kind (`hole.position:holes:12`), a choice the part asks that is not a
//! requirement and has no row (`part.locating`), or an added requirement (`extra.<name>`, a fill
//! of its spec adds it, as a 0.2 answer did); items are characteristic numbers (`#N`). Anything the list does not mention is left alone. A list is all or nothing (Paul,
//! 2026-11-20): if any entry cannot be applied, nothing is written and every failing entry is
//! named with its reason.
//!
//! This module parses and validates; [`crate::writer::apply`] applies a list to a file. The
//! file's state comes through [`FileState`], which the feature table implements.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// The top-level fields of an edit list.
pub const FIELDS: [&str; 6] = [
    "fill",
    "change",
    "items",
    "none",
    "accept_suggestions",
    "clear",
];

/// An edit list, as agreed in the proposal's Appendix.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditList {
    /// Gaps to fill, with a value: `{id: value}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fill: BTreeMap<String, Value>,
    /// Stated feature-and-kinds to change, each with the value the caller saw.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub change: BTreeMap<String, Change>,
    /// Single requirements, by characteristic number.
    #[serde(default, skip_serializing_if = "Items::is_empty")]
    pub items: Items,
    /// Feature-and-kinds decided none.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub none: BTreeSet<String>,
    /// Gaps to fill with Specify's suggestion.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub accept_suggestions: BTreeSet<String>,
    /// Stored decisions that leave no PMI to take back (a decided none, or a choice such as
    /// how holes are located): the feature-and-kind is a gap, or the choice unanswered, again.
    /// PMI is never touched by a clear (that is `change` and `items.remove`).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub clear: BTreeSet<String>,
    /// A 0.2 answers file read as this list ([`from_answers`]): its answers with their
    /// `_specify` metadata, so its face evidence is checked against the file (an answers file
    /// from another revision is refused). Not part of the JSON form.
    #[serde(skip)]
    pub answers_file: Option<Map<String, Value>>,
}

/// A change of a stated feature-and-kind: the new value, and the value the caller saw.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub to: Value,
    pub expect: Value,
}

/// Edits of single requirements, by characteristic number.
///
/// `items.remove` lists characteristic numbers (`"#17"`) and, for what has no number,
/// `{"id": ...}`: a stated row (`{"id": "part.material"}`, every requirement stating it) or an
/// item of the file's other PMI by its id in the feature table (`{"id": "file_pmi:#1234"}`,
/// [`FILE_PMI`]). In memory the two are kept apart: `remove` and `remove_ids`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "ItemsForm", into = "ItemsForm")]
pub struct Items {
    pub change: BTreeMap<Number, ItemChange>,
    pub remove: BTreeSet<Number>,
    /// `items.remove` entries given as `{"id": ...}`: a stated row's id, or a file PMI item's.
    pub remove_ids: BTreeSet<String>,
}

impl Items {
    fn is_empty(&self) -> bool {
        self.change.is_empty() && self.remove.is_empty() && self.remove_ids.is_empty()
    }
}

/// The prefix of a file PMI item's id in the feature table: `file_pmi:#<STEP entity>`, the
/// entity the item's characteristic number is (or would be) attached to. It stays the same
/// while the file is saved in place.
pub const FILE_PMI: &str = "file_pmi:";

/// `items` as JSON: `remove` mixes numbers and `{"id": ...}`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemsForm {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    change: BTreeMap<Number, ItemChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    remove: Vec<Removal>,
}

/// One `items.remove` entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum Removal {
    Number(Number),
    Id(IdRef),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdRef {
    id: String,
}

impl From<ItemsForm> for Items {
    fn from(form: ItemsForm) -> Items {
        let mut items = Items {
            change: form.change,
            ..Items::default()
        };
        for r in form.remove {
            match r {
                Removal::Number(n) => {
                    items.remove.insert(n);
                }
                Removal::Id(IdRef { id }) => {
                    items.remove_ids.insert(id);
                }
            }
        }
        items
    }
}

impl From<Items> for ItemsForm {
    fn from(items: Items) -> ItemsForm {
        ItemsForm {
            change: items.change,
            remove: items
                .remove
                .into_iter()
                .map(Removal::Number)
                .chain(
                    items
                        .remove_ids
                        .into_iter()
                        .map(|id| Removal::Id(IdRef { id })),
                )
                .collect(),
        }
    }
}

/// The entry naming the removal of `id` (`items.remove[{"id":"part.material"}]`).
pub fn removal_entry(id: &str) -> String {
    format!("items.remove[{}]", json!({"id": id}))
}

/// A change of one requirement's tolerance, with the tolerance the caller saw.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemChange {
    pub tolerance: String,
    pub expect: String,
}

/// A characteristic number, written `#N` in the JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Number(pub u64);

impl TryFrom<String> for Number {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        text.strip_prefix('#')
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|n| n.parse().ok())
            .map(Number)
            .ok_or_else(|| {
                format!("{text:?} is not a characteristic number: write it as #N, such as \"#17\"")
            })
    }
}

impl From<Number> for String {
    fn from(n: Number) -> String {
        n.to_string()
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl EditList {
    /// How many entries the list has.
    pub fn len(&self) -> usize {
        self.fill.len()
            + self.change.len()
            + self.items.change.len()
            + self.items.remove.len()
            + self.items.remove_ids.len()
            + self.none.len()
            + self.accept_suggestions.len()
            + self.clear.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ----- the file's state -----

/// What the file says about one feature-and-kind.
#[derive(Clone, Debug, PartialEq)]
pub enum KindState {
    /// The file has a requirement of this kind on this feature. `value` is in the same form a
    /// fill gives it (an answer value: `"0.2"`, `"tapped:M5"`, `[4]`).
    Stated { value: Value },
    /// Nothing is there yet; Specify's suggestion, if it has one.
    Gap { suggestion: Option<Value> },
    /// Someone decided nothing is needed.
    DecidedNone,
}

/// One requirement in the file, found by its characteristic number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement {
    /// Its tolerance as an item change's `expect` gives it (`"±0.1"`).
    pub tolerance: String,
    /// The feature-and-kind it states, or `None` when it sits on no recognised feature
    /// ("file PMI").
    pub feature: Option<String>,
}

/// The file's state, as validation needs it. The feature table implements this.
pub trait FileState {
    /// The state of feature-and-kind `id`, or `None` for an id the file does not have.
    fn kind(&self, id: &str) -> Option<KindState>;
    /// The requirement numbered `number`, or `None` if the file has none by that number.
    fn requirement(&self, number: u64) -> Option<Requirement>;
    /// Whether the file's other PMI has an item with id `id` (`file_pmi:#1234`).
    fn file_item(&self, _id: &str) -> bool {
        false
    }
}

/// A file state held in memory: for tests, and until the feature table exists.
#[derive(Clone, Debug, Default)]
pub struct MemoryState {
    pub kinds: BTreeMap<String, KindState>,
    pub requirements: BTreeMap<u64, Requirement>,
}

impl FileState for MemoryState {
    fn kind(&self, id: &str) -> Option<KindState> {
        self.kinds.get(id).cloned()
    }
    fn requirement(&self, number: u64) -> Option<Requirement> {
        self.requirements.get(&number).cloned()
    }
}

// ----- rules -----

/// Whether two values are the same answer: equal JSON, or the same text (`0.2` and `"0.2"`,
/// blanks around a string ignored).
pub fn same_value(a: &Value, b: &Value) -> bool {
    fn text(v: &Value) -> Option<String> {
        match v {
            Value::String(s) => Some(s.trim().to_owned()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }
    a == b || matches!((text(a), text(b)), (Some(x), Some(y)) if x == y)
}

fn same_for(id: &str, a: &Value, b: &Value) -> bool {
    if id == "part.general_tolerance"
        && let (Some(a), Some(b)) = (a.as_str(), b.as_str())
    {
        return crate::tolerance_block::canonical(a.trim())
            == crate::tolerance_block::canonical(b.trim());
    }
    same_value(a, b)
}

/// Whether a 0.2 answer value means "none is needed": `"none"`, a value starting with the word
/// "None" (`"None (as machined)"`), or an empty list (no other notes).
pub fn is_none_answer(value: &Value) -> bool {
    match value {
        Value::String(s) => {
            let s = s.trim().to_ascii_lowercase();
            s.strip_prefix("none")
                .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
        }
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

// ----- parsing -----

/// An entry that cannot be applied, or a part of the JSON that does not parse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Failure {
    /// The entry, as a path into the list (`change["hole.position:holes:14"]`).
    pub entry: String,
    pub reason: String,
}

fn failure(entry: impl Into<String>, reason: impl Into<String>) -> Failure {
    Failure {
        entry: entry.into(),
        reason: reason.into(),
    }
}

fn key(section: &str, id: &str) -> String {
    format!("{section}[{}]", Value::String(id.to_owned()))
}

/// Parses an edit list, or a 0.2 answers file (flat `{id: value}`, [`from_answers`]). A JSON
/// object is an answers file when it has none of the edit list's fields and every key is a
/// question id (including a standalone measurement) or answers metadata (starts with `_`); anything else is read as an
/// edit list, so a misspelt field is refused rather than taken for an answer.
pub fn read(text: &str) -> Result<EditList, Vec<Failure>> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| vec![failure("(the whole list)", format!("not valid JSON: {e}"))])?;
    if let Value::Object(map) = &value
        && is_answers_file(map)
    {
        return Ok(from_answers(map));
    }
    parse_value(value)
}

pub(crate) fn is_answers_file(map: &Map<String, Value>) -> bool {
    !map.is_empty()
        && map.keys().all(|k| {
            !FIELDS.contains(&k.as_str())
                && (k.contains('.') || k.starts_with('_') || crate::rules::general_rows::is_id(k))
        })
}

/// Parses an edit list. Every part that does not parse is reported, each at its path.
pub fn parse(text: &str) -> Result<EditList, Vec<Failure>> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| vec![failure("(the whole list)", format!("not valid JSON: {e}"))])?;
    parse_value(value)
}

/// [`parse`] of JSON already read.
pub fn parse_value(value: Value) -> Result<EditList, Vec<Failure>> {
    let mut errors = Vec::new();
    let Value::Object(map) = &value else {
        return Err(vec![failure(
            "(the whole list)",
            format!(
                "an edit list is a JSON object with fields {}, not {value}",
                FIELDS.join(", ")
            ),
        )]);
    };
    for (field, v) in map {
        match field.as_str() {
            "fill" => each_in_object(field, v, &mut errors, |path, _, value| {
                value
                    .is_null()
                    .then(|| failure(path, "a fill needs a value; null is not one"))
            }),
            "change" => each_in_object(field, v, &mut errors, |path, _, value| {
                leaf::<Change>(path, value)
            }),
            "items" => items(v, &mut errors),
            "none" | "accept_suggestions" | "clear" => ids(field, v, &mut errors),
            other => errors.push(failure(
                other,
                format!("unknown field; an edit list has {}", FIELDS.join(", ")),
            )),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    serde_json::from_value(value).map_err(|e| vec![failure("(the whole list)", e.to_string())])
}

fn leaf<T: for<'de> Deserialize<'de>>(path: String, value: &Value) -> Option<Failure> {
    serde_json::from_value::<T>(value.clone())
        .err()
        .map(|e| failure(path, e.to_string()))
}

fn each_in_object(
    field: &str,
    v: &Value,
    errors: &mut Vec<Failure>,
    check: impl Fn(String, &str, &Value) -> Option<Failure>,
) {
    let Some(map) = v.as_object() else {
        errors.push(failure(
            field,
            format!("must be a JSON object of id to entry, not {v}"),
        ));
        return;
    };
    for (id, value) in map {
        if id.trim().is_empty() {
            errors.push(failure(key(field, id), "an id cannot be empty"));
        }
        errors.extend(check(key(field, id), id, value));
    }
}

fn ids(field: &str, v: &Value, errors: &mut Vec<Failure>) {
    let Some(list) = v.as_array() else {
        errors.push(failure(field, format!("must be a list of ids, not {v}")));
        return;
    };
    for (i, id) in list.iter().enumerate() {
        if !id.as_str().is_some_and(|s| !s.trim().is_empty()) {
            errors.push(failure(
                format!("{field}[{i}]"),
                format!("must be a feature-and-kind id, such as \"part.coating\", not {id}"),
            ));
        }
    }
}

fn items(v: &Value, errors: &mut Vec<Failure>) {
    let Some(map) = v.as_object() else {
        errors.push(failure(
            "items",
            format!("must be a JSON object with change and remove, not {v}"),
        ));
        return;
    };
    for (field, v) in map {
        match field.as_str() {
            "change" => each_in_object("items.change", v, errors, |path, id, value| {
                Number::try_from(id.to_owned())
                    .err()
                    .map(|why| failure(path.clone(), why))
                    .or_else(|| leaf::<ItemChange>(path, value))
            }),
            "remove" => match v.as_array() {
                Some(list) => {
                    for (i, n) in list.iter().enumerate() {
                        let why = match n {
                            Value::String(s) => Number::try_from(s.to_owned()).err(),
                            Value::Object(o)
                                if o.len() == 1
                                    && o.get("id")
                                        .and_then(Value::as_str)
                                        .is_some_and(|id| !id.trim().is_empty()) =>
                            {
                                None
                            }
                            _ => Some(format!(
                                "{n} is not a characteristic number (#N, such as \"#17\") or \
                                 {{\"id\": ...}} naming a stated row or an item of the file's \
                                 other PMI"
                            )),
                        };
                        errors.extend(why.map(|why| failure(format!("items.remove[{i}]"), why)));
                    }
                }
                None => errors.push(failure(
                    "items.remove",
                    format!("must be a list of characteristic numbers, not {v}"),
                )),
            },
            other => errors.push(failure(
                format!("items.{other}"),
                "unknown field; items has change and remove",
            )),
        }
    }
}

/// A 0.2 answers file (flat `{id: value}`) as an edit list: a fill for each answer, and a
/// none for each answer meaning "none is needed" ([`is_none_answer`]). Metadata (`_specify`,
/// `_specify_characteristic_numbers`: keys starting with `_`) is not an answer.
pub fn from_answers(answers: &Map<String, Value>) -> EditList {
    let mut list = EditList::default();
    for (id, value) in answers {
        if id.starts_with('_') {
            continue;
        }
        if is_none_answer(value) {
            list.none.insert(id.clone());
        } else {
            list.fill.insert(id.clone(), value.clone());
        }
    }
    list.answers_file = Some(answers.clone());
    list
}

// ----- validation -----

/// The result of checking an edit list. Checking never writes anything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Outcome {
    /// The JSON parsed as an edit list (or a 0.2 answers file).
    pub well_formed: bool,
    /// The entries were checked against the file's state, not only against each other.
    pub checked_against_file: bool,
    /// Every entry can be applied (only ever true when checked against the file).
    pub applicable: bool,
    /// How many entries the list has.
    pub entries: usize,
    /// Entries that can be applied but change nothing (a fill equal to what the file states).
    pub unchanged: Vec<String>,
    /// Every entry that cannot be applied, and why.
    pub failures: Vec<Failure>,
}

impl Outcome {
    /// Plain words, for a person or an AI reading the error.
    pub fn message(&self) -> String {
        if !self.failures.is_empty() {
            let what = if self.well_formed {
                format!(
                    "{} of its {} entries cannot be applied",
                    self.failures.len(),
                    self.entries
                )
            } else {
                "it is not a well-formed edit list".to_owned()
            };
            let mut out = format!(
                "Nothing was written. An edit list is applied all or nothing, and {what}, so \
                 none of it was applied: not even the entries that are fine. Fix each entry \
                 below and send the whole list again."
            );
            for f in &self.failures {
                out.push_str(&format!("\n- {}: {}", f.entry, f.reason));
            }
            return out;
        }
        if !self.checked_against_file {
            return format!(
                "The edit list is well formed and its {} entries do not contradict each other, \
                 but it was not checked against the file: this build cannot read the file's \
                 feature table yet. Nothing was written.",
                self.entries
            );
        }
        let unchanged = match self.unchanged.len() {
            0 => String::new(),
            n => format!(" {n} of them change nothing: the file already says the same."),
        };
        format!("All {} entries can be applied.{unchanged}", self.entries)
    }

    /// The outcome as JSON, with its message.
    pub fn to_json(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("an outcome serializes");
        value["written"] = json!(false);
        value["message"] = json!(self.message());
        value
    }

    pub(crate) fn malformed(failures: Vec<Failure>) -> Self {
        Outcome {
            well_formed: false,
            checked_against_file: false,
            applicable: false,
            entries: 0,
            unchanged: Vec::new(),
            failures,
        }
    }
}

/// Reads `text` ([`read`]: an edit list or a 0.2 answers file) and validates it: against
/// `state` when there is one, else only its entries against each other.
pub fn check(text: &str, state: Option<&dyn FileState>) -> Outcome {
    match read(text) {
        Ok(list) => match state {
            Some(state) => validate(&list, state),
            None => validate_alone(&list),
        },
        Err(failures) => Outcome::malformed(failures),
    }
}

/// What can be checked without the file: entries that contradict each other.
pub fn validate_alone(list: &EditList) -> Outcome {
    let failures = contradictions(list);
    Outcome {
        well_formed: true,
        checked_against_file: false,
        applicable: false,
        entries: list.len(),
        unchanged: Vec::new(),
        failures,
    }
}

/// Validates the whole list against the file's state: all applicable, or every failing entry
/// with its reason.
pub fn validate(list: &EditList, state: &dyn FileState) -> Outcome {
    let mut failures = Vec::new();
    let mut unchanged = Vec::new();
    // The requirements removed, by the feature-and-kind they state: removing one and deciding
    // its feature-and-kind none is the proposal's "a gap again unless also decided none".
    let mut removed: BTreeMap<String, Number> = BTreeMap::new();
    // The stated rows removed whole, by id.
    let mut removed_rows: BTreeSet<String> = BTreeSet::new();
    let mut requirements: BTreeMap<Number, Requirement> = BTreeMap::new();
    for &n in list.items.change.keys().chain(&list.items.remove) {
        if let Some(r) = state.requirement(n.0) {
            requirements.insert(n, r);
        }
    }

    for (id, value) in &list.fill {
        let entry = key("fill", id);
        match state.kind(id) {
            None => failures.push(unknown_id(entry, id)),
            Some(KindState::Stated { value: stated }) => {
                // A fill equal to what the file states does nothing (Paul, 2026-11-20).
                if same_for(id, &stated, value) {
                    unchanged.push(entry);
                } else {
                    failures.push(failure(
                        entry,
                        format!(
                            "the file already states {stated} for {id}; to replace it, use \
                             change with \"to\": {value}, \"expect\": {stated}"
                        ),
                    ));
                }
            }
            Some(KindState::Gap { .. } | KindState::DecidedNone) => {}
        }
    }

    for (id, change) in &list.change {
        let entry = key("change", id);
        match state.kind(id) {
            None => failures.push(unknown_id(entry, id)),
            Some(KindState::Stated { value }) => {
                if !same_for(id, &value, &change.expect) {
                    failures.push(failure(
                        entry,
                        format!(
                            "expected {} but the file states {value}: it has changed since you \
                             read it; read it again and send the change with \"expect\": {value}",
                            change.expect
                        ),
                    ));
                }
            }
            Some(KindState::Gap { .. }) => failures.push(failure(
                entry,
                format!(
                    "the file states nothing for {id}, so there is nothing to change; use fill"
                ),
            )),
            Some(KindState::DecidedNone) => failures.push(failure(
                entry,
                format!("{id} is decided none, so there is nothing to change; use fill"),
            )),
        }
    }

    for (n, change) in &list.items.change {
        let entry = key("items.change", &n.to_string());
        match requirements.get(n) {
            None => failures.push(unknown_number(entry, *n)),
            Some(Requirement { feature: None, .. }) => failures.push(failure(
                entry,
                format!(
                    "changing {n} is not supported yet: it sits on no recognised feature. It \
                     can be removed (items.remove), or a stated feature-and-kind changed (change)"
                ),
            )),
            Some(r) if r.tolerance.trim() != change.expect.trim() => failures.push(failure(
                entry,
                format!(
                    "expected {:?} but {n}'s tolerance in the file is {:?}: it has changed since \
                     you read it; read it again and send \"expect\": {:?}",
                    change.expect, r.tolerance, r.tolerance
                ),
            )),
            Some(_) => {}
        }
    }

    for n in &list.items.remove {
        match requirements.get(n) {
            None => failures.push(unknown_number(key("items.remove", &n.to_string()), *n)),
            Some(r) => {
                if let Some(feature) = &r.feature {
                    removed.insert(feature.clone(), *n);
                }
            }
        }
    }

    for id in &list.items.remove_ids {
        let entry = removal_entry(id);
        if id.starts_with(FILE_PMI) {
            if !state.file_item(id) {
                failures.push(failure(
                    entry,
                    format!(
                        "the file's other PMI has no item {id:?}; use an id from the feature \
                         table's file_pmi"
                    ),
                ));
            }
            continue;
        }
        match state.kind(id) {
            None => failures.push(unknown_id(entry, id)),
            Some(KindState::Stated { .. }) => {
                removed_rows.insert(id.clone());
            }
            Some(KindState::Gap { .. }) => failures.push(failure(
                entry,
                format!("the file states nothing for {id}, so there is nothing to remove"),
            )),
            Some(KindState::DecidedNone) => failures.push(failure(
                entry,
                format!("{id} is decided none: the file states nothing for it to remove"),
            )),
        }
    }

    for id in &list.none {
        let entry = key("none", id);
        match state.kind(id) {
            None => failures.push(unknown_id(entry, id)),
            Some(KindState::Stated { value }) => {
                if is_none_answer(&value) {
                    unchanged.push(entry);
                } else if !removed.contains_key(id) && !removed_rows.contains(id) {
                    failures.push(failure(
                        entry,
                        format!(
                            "the file states {value} for {id}; to decide it none, also remove \
                             its requirement by number (items.remove)"
                        ),
                    ));
                }
            }
            Some(KindState::DecidedNone) => unchanged.push(entry),
            Some(KindState::Gap { .. }) => {}
        }
    }

    for id in &list.accept_suggestions {
        let entry = key("accept_suggestions", id);
        match state.kind(id) {
            None => failures.push(unknown_id(entry, id)),
            Some(KindState::Gap {
                suggestion: Some(_),
            }) => {}
            Some(KindState::Gap { suggestion: None }) => failures.push(failure(
                entry,
                format!("Specify has no suggestion for {id}; give a value with fill"),
            )),
            Some(KindState::Stated { value }) => failures.push(failure(
                entry,
                format!(
                    "the file already states {value} for {id}, so there is no gap to fill; to \
                     replace it, use change with \"expect\": {value}"
                ),
            )),
            Some(KindState::DecidedNone) => failures.push(failure(
                entry,
                format!("{id} is decided none and has no suggestion; give a value with fill"),
            )),
        }
    }

    for id in &list.clear {
        let entry = key("clear", id);
        match state.kind(id) {
            None => failures.push(unknown_id(entry, id)),
            Some(KindState::DecidedNone) => {}
            Some(KindState::Gap { .. }) => failures.push(failure(
                entry,
                format!("nothing is stored for {id}, so there is nothing to clear"),
            )),
            Some(KindState::Stated { value }) => failures.push(failure(
                entry,
                format!(
                    "the file states {value} for {id}; a clear takes back only a stored decision \
                     and never touches PMI: use change, or remove its requirement by number \
                     (items.remove)"
                ),
            )),
        }
    }

    failures.extend(contradictions(list));
    failures.extend(item_contradictions(list, &requirements));
    Outcome {
        well_formed: true,
        checked_against_file: true,
        applicable: failures.is_empty(),
        entries: list.len(),
        unchanged,
        failures,
    }
}

fn unknown_id(entry: String, id: &str) -> Failure {
    failure(
        entry,
        format!("the file has no feature-and-kind {id:?}; use an id from the file's feature table"),
    )
}

fn unknown_number(entry: String, n: Number) -> Failure {
    failure(entry, format!("the file has no requirement numbered {n}"))
}

/// The id-level sections an id appears in, by their verbs.
fn sections(list: &EditList) -> BTreeMap<&str, Vec<&'static str>> {
    let mut out: BTreeMap<&str, Vec<&'static str>> = BTreeMap::new();
    for id in list.fill.keys() {
        out.entry(id).or_default().push("fill");
    }
    for id in list.change.keys() {
        out.entry(id).or_default().push("change");
    }
    for id in &list.none {
        out.entry(id).or_default().push("none");
    }
    for id in &list.accept_suggestions {
        out.entry(id).or_default().push("accept_suggestions");
    }
    for id in &list.clear {
        out.entry(id).or_default().push("clear");
    }
    out
}

/// Entries that contradict each other without needing the file: one feature-and-kind in more
/// than one of fill, change, none and accept_suggestions; one requirement both changed and
/// removed.
fn contradictions(list: &EditList) -> Vec<Failure> {
    let mut out = Vec::new();
    for (id, verbs) in sections(list) {
        if verbs.len() > 1 {
            let entries: Vec<String> = verbs.iter().map(|v| key(v, id)).collect();
            out.push(failure(
                entries.join(" and "),
                format!(
                    "the list says {} for the same feature-and-kind {id:?}; keep one of them",
                    verbs.join(" and ")
                ),
            ));
        }
    }
    for n in list.items.change.keys() {
        if list.items.remove.contains(n) {
            out.push(failure(
                format!(
                    "{} and {}",
                    key("items.change", &n.to_string()),
                    key("items.remove", &n.to_string())
                ),
                format!("the list both changes and removes {n}; keep one of them"),
            ));
        }
    }
    out
}

/// A requirement changed or removed by number whose feature-and-kind the list also fills,
/// changes or accepts (or, for a change, decides none): two entries for one requirement.
/// Removing a requirement and deciding its feature-and-kind none is not a contradiction.
fn item_contradictions(
    list: &EditList,
    requirements: &BTreeMap<Number, Requirement>,
) -> Vec<Failure> {
    let by_id = sections(list);
    let mut out = Vec::new();
    let mut check =
        |section: &str, n: &Number, allowed: &[&str]| {
            let Some(feature) = requirements.get(n).and_then(|r| r.feature.as_deref()) else {
                return;
            };
            for verb in by_id.get(feature).into_iter().flatten() {
                if !allowed.contains(verb) {
                    out.push(failure(
                    format!("{} and {}", key(section, &n.to_string()), key(verb, feature)),
                    format!(
                        "{n} is the requirement stating {feature:?}, so both entries address it; \
                         keep one of them"
                    ),
                ));
                }
            }
        };
    for n in list.items.change.keys() {
        check("items.change", n, &[]);
    }
    for n in &list.items.remove {
        check("items.remove", n, &["none"]);
    }
    for id in &list.items.remove_ids {
        for verb in by_id.get(id.as_str()).into_iter().flatten() {
            if *verb != "none" {
                out.push(failure(
                    format!("{} and {}", removal_entry(id), key(verb, id)),
                    format!(
                        "the list both removes {id:?} and says {verb} for it; keep one of them"
                    ),
                ));
            }
        }
    }
    out
}

// ----- the feature table as the file's state -----

/// Whether a kind's answer is a geometric tolerance's magnitude (`hole.position` → `"0.2"`).
pub fn answered_by_tolerance(kind: &str) -> bool {
    matches!(kind, "hole.position" | "diameter.runout")
        || kind.ends_with(".flatness")
        || kind.ends_with(".control")
}

/// The value a stated row has in the form a fill gives it: a datum's faces (`[2]`), a
/// tolerance's magnitude (`"0.2"`) for the kinds answered by one, a part setting's text; else
/// the row's value as the table shows it.
pub fn stated_value(row: &crate::feature_table::Row) -> Value {
    let kind = row.kind.as_str();
    if kind
        .strip_prefix("datum.")
        .is_some_and(|l| !l.contains('.'))
    {
        let faces: BTreeSet<usize> = row
            .requirements
            .iter()
            .filter(|r| r.kind == "datum")
            .flat_map(|r| r.faces.iter().copied())
            .collect();
        if !faces.is_empty() {
            return json!(faces);
        }
    }
    if answered_by_tolerance(kind) {
        let magnitudes: BTreeSet<&str> = row
            .requirements
            .iter()
            .filter(|r| r.kind == "geometric_tolerance")
            .filter_map(|r| r.text.split_whitespace().nth(1))
            // The magnitude alone: `⌀0.3Ⓜ` is answered `0.3` (its zone and modifiers stay
            // as the file has them when it changes).
            .map(|token| token.trim_matches(|c: char| !(c.is_ascii_digit() || c == '.')))
            .filter(|token| !token.is_empty())
            .collect();
        if let [one] = magnitudes.into_iter().collect::<Vec<_>>().as_slice() {
            return Value::String((*one).to_owned());
        }
    }
    row.value.clone().unwrap_or(Value::Null)
}

impl FileState for crate::feature_table::FeatureTable {
    fn kind(&self, id: &str) -> Option<KindState> {
        use crate::feature_table::State;
        let row = self.row(id)?;
        Some(match row.state {
            State::Stated => KindState::Stated {
                value: stated_value(row),
            },
            State::Gap => KindState::Gap {
                suggestion: row.suggestion.clone().filter(|v| !v.is_null()),
            },
            State::DecidedNone => KindState::DecidedNone,
        })
    }

    fn requirement(&self, number: u64) -> Option<Requirement> {
        let tag = Number(number).to_string();
        let numbered = |r: &&crate::feature_table::Requirement| r.number.as_deref() == Some(&tag);
        self.rows
            .iter()
            .find_map(|row| {
                row.requirements.iter().find(numbered).map(|r| Requirement {
                    tolerance: r.text.clone(),
                    feature: Some(row.id.clone()),
                })
            })
            .or_else(|| {
                self.file_pmi.iter().find(numbered).map(|r| Requirement {
                    tolerance: r.text.clone(),
                    feature: None,
                })
            })
    }

    fn file_item(&self, id: &str) -> bool {
        self.file_pmi.iter().any(|r| r.id.as_deref() == Some(id))
    }
}

/// `list` with each `expect` (and each fill) that gives a stated row's value as the table
/// shows it (`"⌖ ⌀0.2 A B"`) given in the fill's form instead (`"0.2"`), so the
/// caller may send back either of the values it saw.
pub fn normalise(list: &EditList, table: &crate::feature_table::FeatureTable) -> EditList {
    let mut out = list.clone();
    let stated = |id: &str, value: &Value| {
        let row = table
            .row(id)
            .filter(|r| r.state == crate::feature_table::State::Stated)?;
        row.value
            .as_ref()
            .filter(|shown| same_for(id, shown, value))
            .map(|_| stated_value(row))
    };
    for (id, change) in &mut out.change {
        if let Some(value) = stated(id, &change.expect) {
            change.expect = value;
        }
    }
    for (id, value) in &mut out.fill {
        if let Some(form) = stated(id, value) {
            *value = form;
        }
    }
    out
}

// ----- the surfaces' schema -----

/// The edit list's JSON Schema, for an MCP tool's `inputSchema` property.
pub fn json_schema() -> Value {
    let ids = |what: &str| json!({"type": "array", "items": {"type": "string", "minLength": 1}, "uniqueItems": true, "description": what});
    let number = json!({"type": "string", "pattern": "^#[0-9]+$"});
    json!({
        "type": "object",
        "description": "Edits to apply to the file, all or nothing: if any entry cannot be applied, nothing is written and every failing entry is named. Ids are feature-and-kind (hole.position:holes:12), a choice with no row (part.locating), or an added requirement (extra.<name>, filled with its spec); items are characteristic numbers (#17). Anything not mentioned is left alone.",
        "properties": {
            "fill": {
                "type": "object",
                "description": "Gaps to fill: {id: value}. A fill equal to what the file states changes nothing; one that differs is refused (use change).",
                "additionalProperties": {"not": {"type": "null"}}
            },
            "change": {
                "type": "object",
                "description": "Stated feature-and-kinds to change: {id: {to, expect}}, expect being the value you saw. Refused if the file has moved on.",
                "additionalProperties": {
                    "type": "object",
                    "properties": {"to": {}, "expect": {}},
                    "required": ["to", "expect"],
                    "additionalProperties": false
                }
            },
            "items": {
                "type": "object",
                "description": "Single requirements by characteristic number.",
                "properties": {
                    "change": {
                        "type": "object",
                        "description": "{\"#N\": {tolerance, expect}}: a requirement's new tolerance and the one you saw. Not yet for a requirement on no recognised feature.",
                        "propertyNames": number,
                        "additionalProperties": {
                            "type": "object",
                            "properties": {"tolerance": {"type": "string"}, "expect": {"type": "string"}},
                            "required": ["tolerance", "expect"],
                            "additionalProperties": false
                        }
                    },
                    "remove": {"type": "array", "items": {"oneOf": [number, {"type": "object", "properties": {"id": {"type": "string", "minLength": 1}}, "required": ["id"], "additionalProperties": false}]}, "uniqueItems": true, "description": "Requirements to remove: a characteristic number (#17), or {\"id\": ...} for what has none: a stated row (part.material, every requirement stating it) or an item of the file's other PMI by its feature-table id (file_pmi:#1234). Their numbers are retired."}
                },
                "additionalProperties": false
            },
            "none": ids("Feature-and-kinds decided none (nothing is needed)."),
            "accept_suggestions": ids("Gaps to fill with Specify's suggestion."),
            "clear": ids("Stored decisions that leave no PMI to take back (a decided none, or a choice such as part.locating): the feature-and-kind is a gap, or the choice unanswered, again. Never touches PMI (use change or items.remove).")
        },
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_appendix_list_round_trips() {
        let text = r##"{
          "fill":   {"hole.position:holes:12": "0.2"},
          "change": {"hole.position:holes:14": {"to": "0.1", "expect": "0.2"}},
          "items":  {"change": {"#21": {"tolerance": "±0.05", "expect": "±0.1"}},
                     "remove": ["#17"]},
          "none":   ["part.coating", "datum.A.flatness"],
          "accept_suggestions": ["hole.size:holes:3"],
          "clear": ["part.locating"]
        }"##;
        let list = parse(text).unwrap();
        assert_eq!(list.len(), 8);
        assert!(list.items.remove.contains(&Number(17)));
        let again: EditList = serde_json::from_value(serde_json::to_value(&list).unwrap()).unwrap();
        assert_eq!(again, list);
    }

    #[test]
    fn a_removal_is_a_number_or_an_id() {
        let text = r##"{"items": {"remove": ["#17", {"id": "part.material"}, {"id": "file_pmi:#1234"}]}}"##;
        let list = parse(text).unwrap();
        assert_eq!(list.len(), 3);
        assert!(list.items.remove.contains(&Number(17)));
        assert!(list.items.remove_ids.contains("part.material"));
        let again: EditList = serde_json::from_value(serde_json::to_value(&list).unwrap()).unwrap();
        assert_eq!(again, list);
        for bad in [
            r#"{"id": ""}"#,
            r#"{"id": "x", "more": 1}"#,
            "17",
            r##"{"number": "#17"}"##,
        ] {
            let text = format!(r#"{{"items": {{"remove": [{bad}]}}}}"#);
            let failures = parse(&text).unwrap_err();
            assert_eq!(failures[0].entry, "items.remove[0]", "{bad}: {failures:?}");
        }
    }

    #[test]
    fn the_schema_names_the_structs_fields() {
        let schema = json_schema();
        let fields: Vec<&str> = schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(fields, FIELDS);
    }
}
