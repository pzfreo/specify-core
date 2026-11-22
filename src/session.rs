//! A Specify session (docs/session.md): one opened STEP file (or, for a diff, the pair), its
//! answers by part, a revision number `rev` that goes up by one on every change, the recent
//! changes, and the pages listening for them. The MCP server (`specify-core mcp`) holds one per
//! opened file and the page server (src/open.rs) serves one, so the agent and the page edit one
//! set of answers; `specify-core open` holds one with no agent.
//!
//! Save applies an edit list (ADR P11, src/edits.rs: all or nothing, touching only what it
//! names) to the file as the session's last save left it, with the same writer as
//! `specify-core write STEP EDITS.json`, beside the file under the first free
//! `<stem>.specified.step` (later saves update that copy while it is as the session left it),
//! or over it when opened in place (the original kept once as `.bak`). The session's answers
//! include the decisions stored in the file and draft answers. The agent can save both its
//! drafts and explicit edits together, and the page submits its visible drafts in its lists.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::edits::{self, EditList, Failure, Outcome};
use sha2::{Digest, Sha256};

use crate::api::{self, Error};
use crate::part::{self, LoadError};

/// The changes kept for `changes since`; an older `since` gets the whole answers instead.
pub const KEPT_CHANGES: usize = 256;

/// The most events a subscriber may fall behind by; one further behind is dropped (its stream
/// ends, and a page reconnecting gets the whole state again).
pub const MAX_BEHIND: usize = 1000;

/// A session shared between the MCP server and the page server it started.
pub type Shared = Arc<Mutex<Session>>;

/// Locks a session, whatever a panicking holder left (its state is always whole).
pub fn lock(session: &Shared) -> std::sync::MutexGuard<'_, Session> {
    session.lock().unwrap_or_else(|p| p.into_inner())
}

/// The value of the feature table's row `id`, when the file states it.
pub(crate) fn stated_value<'a>(analysis: &'a Value, id: &str) -> Option<&'a Value> {
    analysis["table"]["rows"]
        .as_array()?
        .iter()
        .find(|row| row["id"] == id && row["state"] == "stated")
        .map(|row| row.get("value").unwrap_or(&Value::Null))
}

/// A file as opened: its path (absolute, as given or as the caller chose), its name and its
/// bytes as they were on disk (gzipped if the file is).
#[derive(Clone)]
pub struct Opened {
    pub path: PathBuf,
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
}

impl Opened {
    /// `path` made absolute (not resolved: a link stays the link) and read.
    pub fn read(path: &Path) -> Result<Opened, Error> {
        let unreadable = |e: io::Error| {
            Error::Load(LoadError::Unreadable {
                path: path.to_path_buf(),
                reason: e.to_string(),
            })
        };
        let path = std::path::absolute(path).map_err(unreadable)?;
        let bytes = fs::read(&path).map_err(unreadable)?;
        Ok(Opened::of(path, bytes))
    }

    /// A file already read from `path`.
    pub fn of(path: PathBuf, bytes: Vec<u8>) -> Opened {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Opened {
            path,
            name,
            bytes: Arc::new(bytes),
        }
    }
}

/// One change to the answers: `set` the answers that are new or different, `cleared` the ids
/// no longer answered.
#[derive(Clone, Debug)]
pub struct Change {
    pub rev: u64,
    pub part: usize,
    pub source: &'static str,
    pub set: Map<String, Value>,
    pub cleared: Vec<String>,
}

impl Change {
    pub fn to_json(&self) -> Value {
        json!({"rev": self.rev, "part": self.part, "source": self.source,
               "set": self.set, "cleared": self.cleared})
    }
}

/// A save written: where, how many bytes, at which `rev`, `write`'s report, and whether any
/// required question is still open.
#[derive(Clone, Debug)]
pub struct Saved {
    pub path: String,
    pub bytes: usize,
    pub rev: u64,
    pub report: Value,
    /// The session answers at the revision acknowledged by the page save.
    pub parts: Value,
    pub complete: bool,
    pub missing: Vec<String>,
}

impl Saved {
    pub fn to_json(&self) -> Value {
        json!({"path": self.path, "bytes": self.bytes, "rev": self.rev, "report": self.report,
               "complete": self.complete, "missing": self.missing})
    }
}

/// Why nothing was saved.
#[derive(Debug)]
pub enum SaveError {
    /// The session has moved on from the rev asked for (its current rev).
    Stale(u64),
    /// The save would write outside the allowed directories.
    Outside(String),
    /// A default approved that is not an open question with a default on its part, or
    /// (`expected`, `current`) whose value is no longer its default.
    Question {
        message: String,
        part: usize,
        question: String,
        /// The value approved and the default now, when they differ.
        values: Option<Box<(Value, Value)>>,
    },
    /// An in-place save over a file changed on disk since it was opened (by anyone but this
    /// session).
    Changed(String),
    /// An answer the writer refuses (the message names the question).
    Refused(String),
    /// An edit list that cannot be applied, all or nothing: every failing entry and why.
    Edits(Vec<crate::edits::Failure>),
    /// A batch's refused entries with their part indices.
    PartEdits(Vec<PartFailure>),
    /// The file could not be written.
    Failed(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Stale(rev) => write!(f, "the answers have moved on to rev {rev}"),
            SaveError::Question { message, .. } => f.write_str(message),
            SaveError::Edits(failures) => f.write_str(&edits_refused(failures)),
            SaveError::PartEdits(failures) => f.write_str(&part_edits_refused(failures)),
            SaveError::Changed(m)
            | SaveError::Outside(m)
            | SaveError::Refused(m)
            | SaveError::Failed(m) => f.write_str(m),
        }
    }
}

/// One opened file's answers and what listens to them.
pub struct Session {
    file: Opened,
    /// SHA-256 of the file's bytes as opened.
    sha: String,
    /// `diff` mode: the old revision.
    old: Option<Opened>,
    /// `diff` mode: the comparison as WASM `diff()` gives it (`{report, markdown, view}`, or
    /// for an assembly `{report, markdown, parts}`), made once when the session starts.
    compared: Option<Value>,
    in_place: bool,
    gz: bool,
    /// The file, parsed from its bytes as opened (every save writes from these).
    step: Arc<part::File>,
    /// What `file` serves: the input, or what the last in-place save left.
    current: Arc<Vec<u8>>,
    /// In place: where the first save kept the original.
    backup: Option<PathBuf>,
    /// In place: SHA-256 of each file this session's saves wrote, so the file is not taken as
    /// changed (recorded before the file is renamed into place).
    written: Vec<String>,
    /// The file the last successful save wrote (as the save reports its path) and SHA-256 of
    /// the bytes it stored there (`GET file?rev=saved`).
    last_saved: Option<(PathBuf, String)>,
    /// The directories saves may write in (MCP's `--allow`); None: anywhere beside the file.
    allowed: Option<Vec<PathBuf>>,
    analyses: BTreeMap<usize, Arc<Value>>,
    answers: BTreeMap<usize, Map<String, Value>>,
    rev: u64,
    changes: VecDeque<Change>,
    /// The newest `rev` whose change is no longer kept (0: none dropped).
    dropped: u64,
    subscribers: Vec<mpsc::SyncSender<String>>,
    /// One save at a time, including staging outside the session lock.
    save_lock: Arc<Mutex<()>>,
    /// Each part's answers as the file last saved (or opened) holds them: answers that differ
    /// are unsaved.
    saved: BTreeMap<usize, Map<String, Value>>,
    /// Tests only: called by a save just before it acquires the final commit lock.
    #[doc(hidden)]
    pub between_locks: Option<fn(&Shared)>,
    /// Tests only: called after the final revision check, immediately before disk commit.
    #[doc(hidden)]
    pub commit_boundary: Option<fn(&Shared)>,
    /// The page server serving this session, if one was started.
    pub page: Option<crate::open::Page>,
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Session {
    /// A session of `file` with no answers yet.
    pub fn new(file: Opened, in_place: bool) -> Result<Session, Error> {
        let step = part::File::of_shared(&file.path, Arc::clone(&file.bytes))?;
        Ok(Session::of(file, step, in_place))
    }

    /// [`Session::new`] of a file the caller has parsed from `file`'s bytes.
    pub fn of(file: Opened, step: part::File, in_place: bool) -> Session {
        let gz = file.name.to_ascii_lowercase().ends_with(".gz");
        Session {
            sha: sha256(&file.bytes),
            current: Arc::clone(&file.bytes),
            file,
            old: None,
            compared: None,
            in_place,
            gz,
            step: Arc::new(step),
            backup: None,
            written: Vec::new(),
            last_saved: None,
            allowed: None,
            analyses: BTreeMap::new(),
            answers: BTreeMap::new(),
            rev: 0,
            changes: VecDeque::new(),
            dropped: 0,
            subscribers: Vec::new(),
            save_lock: Arc::new(Mutex::new(())),
            saved: BTreeMap::new(),
            between_locks: None,
            commit_boundary: None,
            page: None,
        }
    }

    /// `specify-core open FILE`: a session starting from the answers stored in the file (a
    /// file Specify wrote resumes them, part by part, as `analyse` reports them).
    pub fn open(file: Opened, in_place: bool) -> Result<Session, Error> {
        let mut session = Session::new(file, in_place)?;
        session.resume()?;
        Ok(session)
    }

    /// Starts every part the file stores answers for (a file Specify wrote) from them, as
    /// `analyse` resumes them. For a session just made, before anyone has seen it.
    pub fn resume(&mut self) -> Result<(), Error> {
        let written: Vec<usize> = api::parts_of(&self.step)
            .iter()
            .filter(|p| p["written"] == json!(true))
            .filter_map(|p| p["part"].as_u64().map(|k| k as usize))
            .collect();
        for k in written {
            let analysis = self.analysis(k)?;
            if let Some(resumed) = analysis["resume"]["answers"].as_object() {
                self.answers.entry(k).or_insert_with(|| resumed.clone());
            }
        }
        self.saved = self.answers.clone();
        Ok(())
    }

    /// The working edits on one part, only where its answers differ from the saved file.
    fn draft(&self, part: usize) -> EditList {
        let empty = Map::new();
        let current = self.answers.get(&part).unwrap_or(&empty);
        let saved = self.saved.get(&part).unwrap_or(&empty);
        let mut draft = EditList::default();
        for (id, value) in current {
            if id.starts_with('_') || saved.get(id) == Some(value) {
                continue;
            }
            if let Some(expect) = self.analyses.get(&part).and_then(|a| stated_value(a, id)) {
                draft.change.insert(
                    id.clone(),
                    edits::Change {
                        to: value.clone(),
                        expect: expect.clone(),
                    },
                );
            } else if edits::is_none_answer(value) {
                draft.none.insert(id.clone());
            } else {
                draft.fill.insert(id.clone(), value.clone());
            }
        }
        draft.clear.extend(
            saved
                .keys()
                .filter(|id| !id.starts_with('_') && !current.contains_key(*id))
                .cloned(),
        );
        draft
    }

    fn drafts(&self) -> BTreeMap<usize, EditList> {
        self.answers
            .keys()
            .filter_map(|&part| {
                let draft = self.draft(part);
                (!draft.is_empty()).then_some((part, draft))
            })
            .collect()
    }

    pub(crate) fn answer_snapshot(&self) -> Value {
        let mut snapshot = json!({"rev": self.rev, "parts": self.parts()});
        let drafts = self.drafts();
        if !drafts.is_empty() {
            snapshot["drafts"] = json!(drafts);
        }
        snapshot
    }

    /// `specify-core open --diff OLD NEW` (and MCP's `open` with `old`): `diff OLD NEW` compared
    /// as `specify-core diff` compares them (part `part`, or the files' only parts, or their
    /// assemblies), from the PMI in the two files only. The session is NEW's, as `open` starts
    /// it: the caller resumes it.
    pub fn diff(
        old: Opened,
        new: Opened,
        part: Option<usize>,
        in_place: bool,
    ) -> Result<Session, Error> {
        let mut session = Session::new(new, in_place)?;
        let old_file = part::File::of_shared(&old.path, Arc::clone(&old.bytes))?;
        let compared = crate::revision::compare(
            &old_file,
            &session.step,
            part,
            // With the PMI scenes, as WASM `diff(..., scenes: true)`: the page draws PMI
            // changes from them without comparing again.
            crate::revision::DiffOptions { scenes: true },
        )?;
        session.compared = Some(compared.diff_json(&session.file.name));
        session.old = Some(old);
        Ok(session)
    }

    pub fn file(&self) -> &Opened {
        &self.file
    }

    pub fn old(&self) -> Option<&Opened> {
        self.old.as_ref()
    }

    /// `diff` mode: the revision report, as `diff`'s report.json (without the PMI scenes the
    /// page's `GET diff` carries).
    pub fn report(&self) -> Option<Value> {
        self.compared
            .as_ref()
            .map(|c| without_scenes(c["report"].clone()))
    }

    /// `diff` mode: the comparison for the page's `GET diff`, as WASM `diff()` gives it.
    pub fn comparison(&self) -> Option<&Value> {
        self.compared.as_ref()
    }

    pub fn in_place(&self) -> bool {
        self.in_place
    }

    pub fn set_in_place(&mut self, in_place: bool) {
        self.in_place = in_place;
    }

    /// SHA-256 of the file's bytes as opened.
    pub fn sha(&self) -> &str {
        &self.sha
    }

    /// Whether bytes with this SHA-256 are the file as opened, or as this session's own last
    /// in-place save left it.
    pub fn is_ours(&self, sha: &str) -> bool {
        sha == self.sha || self.written.iter().any(|w| w == sha)
    }

    /// What `file` serves: the input as opened, or as the last in-place save left it.
    pub fn current(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.current)
    }

    /// The file as opened, parsed.
    pub fn step(&self) -> &part::File {
        &self.step
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Part `part`'s analysis, made the first time it is asked for (the part is then open in
    /// the session).
    pub fn analysis(&mut self, part: usize) -> Result<Arc<Value>, Error> {
        if let Some(a) = self.analyses.get(&part) {
            return Ok(Arc::clone(a));
        }
        let analysis = Arc::new(analyse(&self.step, part)?);
        self.analyses.insert(part, Arc::clone(&analysis));
        Ok(analysis)
    }

    /// Part `part`'s analysis, made by the caller (MCP's `open`).
    pub fn keep_analysis(&mut self, part: usize, analysis: Arc<Value>) {
        self.analyses.entry(part).or_insert(analysis);
    }

    /// The parts the file has.
    pub fn part_count(&self) -> usize {
        self.step.definitions().len()
    }

    /// Part `part`'s answers, if the session holds any for it.
    pub fn answers(&self, part: usize) -> Option<&Map<String, Value>> {
        self.answers.get(&part)
    }

    /// Every part's answers, by part index as a string: `answers`' `parts`.
    pub fn parts(&self) -> Value {
        Value::Object(
            self.answers
                .iter()
                .map(|(k, a)| (k.to_string(), Value::Object(a.clone())))
                .collect(),
        )
    }

    /// Starts part `part` from `answers` without recording a change (a session just made,
    /// before anyone has seen it).
    pub fn start_answers(&mut self, part: usize, answers: Map<String, Value>) {
        self.answers.insert(part, answers);
    }

    /// Replaces part `part`'s answers: the new `rev` and the change, or None when nothing
    /// changed. Every subscriber gets an `answers` event.
    pub fn set(
        &mut self,
        part: usize,
        answers: Map<String, Value>,
        source: &'static str,
    ) -> Option<u64> {
        self.change(part, answers, source, None)
    }

    /// [`Session::set`], at `rev` when several parts change together (else the next rev).
    fn change(
        &mut self,
        part: usize,
        answers: Map<String, Value>,
        source: &'static str,
        at: Option<u64>,
    ) -> Option<u64> {
        let before = self.answers.get(&part);
        // A part joining the session is a change even with no answers: a page holding an older
        // rev then re-reads, and sees it.
        let joined = before.is_none();
        if before == Some(&answers) {
            return None;
        }
        let empty = Map::new();
        let before = before.unwrap_or(&empty);
        let set: Map<String, Value> = answers
            .iter()
            .filter(|(id, v)| before.get(*id) != Some(*v))
            .map(|(id, v)| (id.clone(), v.clone()))
            .collect();
        let cleared: Vec<String> = before
            .keys()
            .filter(|id| !answers.contains_key(*id))
            .cloned()
            .collect();
        self.rev = at.unwrap_or(self.rev + 1);
        let rev = self.rev;
        self.changes.push_back(Change {
            rev,
            part,
            source,
            set,
            cleared,
        });
        while self.changes.len() > KEPT_CHANGES {
            if let Some(gone) = self.changes.pop_front() {
                self.dropped = gone.rev;
            }
        }
        let mut data = json!({"rev": rev, "part": part, "answers": answers, "source": source});
        if joined {
            data["joined"] = json!(true);
        }
        self.answers.insert(part, answers);
        let draft = self.draft(part);
        if at.is_some() {
            // Saves and whole-file discards publish several part events at one revision.
            // Clear the draft bar on the first event, even before the other parts arrive.
            data["drafts"] = json!({});
        } else if !draft.is_empty() {
            data["draft"] = json!(draft);
        }
        self.publish(&event("answers", Some(rev), &data));
        Some(rev)
    }

    /// The changes after `since`, oldest first; None when some of them are no longer kept.
    pub fn changes_since(&self, since: u64) -> Option<Vec<&Change>> {
        if since < self.dropped {
            return None;
        }
        Some(self.changes.iter().filter(|c| c.rev > since).collect())
    }

    /// A new subscriber: its first event is the current state (`source: "start"`), then one
    /// per change and save.
    pub fn subscribe(&mut self) -> mpsc::Receiver<String> {
        let (tell, events) = mpsc::sync_channel(MAX_BEHIND + 1);
        let mut data = self.answer_snapshot();
        data["source"] = json!("start");
        let _ = tell.send(event("answers", Some(self.rev), &data));
        self.subscribers.push(tell);
        events
    }

    fn publish(&mut self, text: &str) {
        // A subscriber too far behind (or gone) is dropped: its stream ends.
        self.subscribers
            .retain(|s| s.try_send(text.to_string()).is_ok());
    }

    /// Where the next save goes: the input (in place), the copy this session's last save wrote
    /// while it is as that save left it, or the first free `<stem>.specified.step` beside it.
    pub fn save_target(&self) -> Option<PathBuf> {
        if self.in_place {
            return Some(self.file.path.clone());
        }
        if let Some(copy) = self.own_copy() {
            return Some(copy.to_path_buf());
        }
        let dir = self.file.path.parent()?;
        let stem = stem(&self.file.name);
        (1..=MAX_CANDIDATES)
            .map(|n| candidate(dir, stem, n))
            .find(|p| fs::symlink_metadata(p).is_err())
    }

    /// Not in place: the copy this session's last save wrote, while it is still a plain file
    /// holding exactly what that save stored (#217). A later save updates it; a copy removed,
    /// replaced or changed by anyone else is never written over (the save takes a new name).
    fn own_copy(&self) -> Option<&Path> {
        let (path, sha) = self.last_saved.as_ref().filter(|_| !self.in_place)?;
        let plain = fs::symlink_metadata(path).is_ok_and(|m| m.is_file());
        (plain && fs::read(path).is_ok_and(|b| sha256(&b) == *sha)).then_some(path.as_path())
    }

    /// The file the last successful save in this session wrote and SHA-256 of the bytes it
    /// stored there; None before any save.
    pub fn last_saved(&self) -> Option<(&Path, &str)> {
        self.last_saved
            .as_ref()
            .map(|(path, sha)| (path.as_path(), sha.as_str()))
    }

    /// Saves write only inside these directories (canonical), as MCP's `--allow` says.
    pub fn set_allowed(&mut self, dirs: Vec<PathBuf>) {
        self.allowed = Some(dirs);
    }

    /// Refuses a save that would write outside the allowed directories: beside the file as
    /// given, or (in place) over the file it leads to.
    fn check_allowed(&self) -> Result<(), SaveError> {
        let Some(dirs) = &self.allowed else {
            return Ok(());
        };
        let path = &self.file.path;
        let failed = |e: io::Error| SaveError::Failed(format!("{}: {e}", path.display()));
        let dir = path.parent().unwrap_or(Path::new("/"));
        let mut written = vec![dir.canonicalize().map_err(failed)?];
        if self.in_place {
            written.push(path.canonicalize().map_err(failed)?);
        }
        if written
            .iter()
            .all(|w| dirs.iter().any(|d| w.starts_with(d)))
        {
            Ok(())
        } else {
            Err(SaveError::Outside(format!(
                "{} is outside the directories this server may write in: nothing is saved",
                path.display()
            )))
        }
    }

    /// Whether part `part` has been analysed (is open in the session).
    pub fn is_open(&self, part: usize) -> bool {
        self.analyses.contains_key(&part)
    }
}

/// Part `part`'s analysis, made without holding the session's lock (and kept: the part is
/// then open in the session).
pub fn analysis(shared: &Shared, part: usize) -> Result<Arc<Value>, Error> {
    let step = {
        let s = lock(shared);
        if let Some(a) = s.analyses.get(&part) {
            return Ok(Arc::clone(a));
        }
        Arc::clone(&s.step)
    };
    let made = Arc::new(analyse(&step, part)?);
    let mut s = lock(shared);
    Ok(Arc::clone(s.analyses.entry(part).or_insert(made)))
}

/// Part `part` of `step`, analysed.
fn analyse(step: &part::File, part: usize) -> Result<Value, Error> {
    let loaded = step.load(Some(part))?;
    Ok(api::analyse_reporting_in(step, &loaded)?.0)
}

/// Drops working answers without writing. An individual discard restores that row to the
/// file's saved value, and a whole-file discard restores every part at one new revision.
pub fn discard_answers(
    shared: &Shared,
    rev: Option<u64>,
    target: Option<(usize, &str)>,
    source: &'static str,
) -> Result<Value, SaveError> {
    let mut s = lock(shared);
    if rev.is_some_and(|rev| rev != s.rev) {
        return Err(SaveError::Stale(s.rev));
    }
    let at = s.rev + 1;
    if let Some((part, id)) = target {
        if part >= s.part_count() || id.starts_with('_') {
            return Err(SaveError::Refused("invalid part or question id".into()));
        }
        if let Some(mut current) = s.answers.get(&part).cloned() {
            match s.saved.get(&part).and_then(|answers| answers.get(id)) {
                Some(value) => {
                    current.insert(id.to_owned(), value.clone());
                }
                None => {
                    current.remove(id);
                }
            }
            s.change(part, current, source, None);
        }
    } else {
        let parts: Vec<_> = s.answers.keys().copied().collect();
        for part in parts {
            let saved = s.saved.get(&part).cloned().unwrap_or_default();
            s.change(part, saved, source, Some(at));
        }
    }
    Ok(s.answer_snapshot())
}

/// Applies the edit list `list` (ADR P11: all or nothing, touching only what it names) to the
/// session's file as its last save left it (the input before any), and saves the result:
/// in place, to the session's own copy, or beside the input under the first free
/// `<stem>.specified.step`. `rev` (None: any) is the session revision the caller last saw: a
/// session that has moved on is refused as [`SaveError::Stale`]. A list that cannot be applied
/// writes nothing, with every failing entry named. MCP adds the working answers to its lists,
/// and the page submits or explicitly discards every draft. The session then uses the saved file: its
/// analyses are made again, and each part's answers are those the saved file resumes (a new
/// rev). Every subscriber gets a `saved` event. A file of several parts (or of a part placed more than
/// once) takes [`save_part_edits`].
pub fn save_edits(
    shared: &Shared,
    rev: Option<u64>,
    list: &crate::edits::EditList,
    source: &'static str,
) -> Result<Saved, SaveError> {
    save_part_edits(shared, None, rev, list, source)
}

/// [`save_edits`] on part `part` of the session's file (None: a file of one part), as
/// `write STEP EDITS.json --part K` applies it: in an assembly only that part is edited, once
/// however often it is placed, and every other part, placement and their PMI keep their bytes
/// (decision D3).
pub fn save_part_edits(
    shared: &Shared,
    part: Option<usize>,
    rev: Option<u64>,
    list: &crate::edits::EditList,
    source: &'static str,
) -> Result<Saved, SaveError> {
    save_with(shared, rev, source, |step, drafts| {
        let loaded = step
            .load(part)
            .map_err(|e| SaveError::Refused(e.to_string()))?;
        let index = loaded.binding.part;
        let lists = include_drafts(
            BTreeMap::from([(index, list.clone())]),
            drafts,
            &step,
            source,
        )?;
        if lists.len() == 1 {
            let applied =
                crate::writer::apply::apply(&loaded, &lists[&index]).map_err(SaveError::Edits)?;
            let report = applied.to_dict();
            Ok((Arc::new(applied.bytes), report, applied.unchanged))
        } else {
            stage_assembly_edits(step, &lists).map(StagedAssembly::save)
        }
    })
}

/// A failure from a part list. Keep the existing qualified diagnostic in `entry` and give
/// callers the index and raw edit path separately for routing errors to a row.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartFailure {
    part: usize,
    entry: String,
    edit_entry: String,
    reason: String,
}

impl PartFailure {
    fn new(part: usize, label: &str, failure: Failure) -> Self {
        Self {
            part,
            entry: format!("{label}: {}", failure.entry),
            edit_entry: failure.entry,
            reason: failure.reason,
        }
    }

    fn plain(&self) -> Failure {
        Failure {
            entry: self.entry.clone(),
            reason: self.reason.clone(),
        }
    }
}

pub(crate) fn part_edits_refused(failures: &[PartFailure]) -> String {
    edits_refused(&failures.iter().map(PartFailure::plain).collect::<Vec<_>>())
}

fn part_label(parts: &[part::PartSummary], part: usize) -> String {
    match parts.get(part) {
        Some(item) => format!("Part {:?} ({part})", item.name),
        None => format!("Part {part}"),
    }
}

fn mentions(list: &EditList, id: &str) -> bool {
    list.fill.contains_key(id)
        || list.change.contains_key(id)
        || list.none.contains(id)
        || list.clear.contains(id)
        || list.accept_suggestions.contains(id)
        || list.items.remove_ids.contains(id)
}

fn include_drafts(
    mut lists: BTreeMap<usize, EditList>,
    drafts: &BTreeMap<usize, EditList>,
    step: &part::File,
    source: &str,
) -> Result<BTreeMap<usize, EditList>, SaveError> {
    let mut failures = Vec::new();
    let parts = if drafts.is_empty() || source == "mcp" {
        Vec::new()
    } else {
        step.parts()
    };
    for (&part, draft) in drafts {
        if source == "mcp" {
            let list = lists.entry(part).or_default();
            let mut draft = draft.clone();
            draft.fill.retain(|id, _| !mentions(list, id));
            draft.change.retain(|id, _| !mentions(list, id));
            draft.none.retain(|id| !mentions(list, id));
            draft.clear.retain(|id| !mentions(list, id));
            list.fill.extend(draft.fill);
            list.change.extend(draft.change);
            list.none.extend(draft.none);
            list.clear.extend(draft.clear);
        } else {
            let label = part_label(&parts, part);
            for (section, id) in draft
                .fill
                .keys()
                .map(|id| ("fill", id))
                .chain(draft.change.keys().map(|id| ("change", id)))
                .chain(draft.none.iter().map(|id| ("none", id)))
                .chain(draft.clear.iter().map(|id| ("clear", id)))
            {
                if !lists.get(&part).is_some_and(|list| mentions(list, id)) {
                    failures.push(PartFailure::new(
                        part,
                        &label,
                        Failure {
                            entry: format!("{section}[{}]", json!(id)),
                            reason:
                                "include the staged answer in the list, or discard it before saving"
                                    .into(),
                        },
                    ));
                }
            }
        }
    }
    if failures.is_empty() {
        Ok(lists)
    } else {
        Err(SaveError::PartEdits(failures))
    }
}

/// A malformed assembly edit request. Page and MCP callers share its shape and entry parsing.
pub(crate) enum PartEditsError {
    Shape(String),
    List(Vec<PartFailure>),
}

pub(crate) fn read_part_edits(value: &Value) -> Result<BTreeMap<usize, EditList>, PartEditsError> {
    let bad = |why: &str| PartEditsError::Shape(why.into());
    let parts = value
        .as_array()
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| bad("partEdits must be a nonempty array of {part, edits}"))?;
    let mut lists = BTreeMap::new();
    for entry in parts {
        let part = entry
            .get("part")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| bad("each partEdits entry requires a nonnegative part index"))?;
        let list = entry
            .get("edits")
            .filter(|e| e.is_object())
            .ok_or_else(|| {
                bad(&format!(
                    "Part {part} requires an edits object ({{}} for none)"
                ))
            })?;
        let list = edits::read(&list.to_string()).map_err(|failures| {
            let label = format!("Part {part}");
            PartEditsError::List(
                failures
                    .into_iter()
                    .map(|f| PartFailure::new(part, &label, f))
                    .collect(),
            )
        })?;
        if lists.insert(part, list).is_some() {
            return Err(bad(&format!("partEdits repeats part {part}")));
        }
    }
    Ok(lists)
}

struct StagedAssembly {
    bytes: Arc<Vec<u8>>,
    reports: Vec<Value>,
    unchanged: bool,
    unchanged_entries: Vec<String>,
}

impl StagedAssembly {
    fn save(self) -> (Arc<Vec<u8>>, Value, bool) {
        (
            self.bytes,
            json!({"parts": self.reports, "unchanged": self.unchanged}),
            self.unchanged,
        )
    }
}

/// Runs each list through the assembly writer on the staged result of the preceding lists.
/// The same staging drives both saves and dry-run checks.
fn stage_assembly_edits(
    mut step: Arc<part::File>,
    lists: &BTreeMap<usize, EditList>,
) -> Result<StagedAssembly, SaveError> {
    if lists.is_empty() {
        return Err(SaveError::Refused("no part lists to apply".into()));
    }
    let parts = step.parts();
    let mut failures = Vec::new();
    let mut staged = StagedAssembly {
        bytes: Arc::clone(step.bytes()),
        reports: Vec::new(),
        unchanged: true,
        unchanged_entries: Vec::new(),
    };
    for (&part, list) in lists {
        let name = parts.get(part).map(|p| p.name.as_str());
        let label = part_label(&parts, part);
        let result = step
            .load(Some(part))
            .map_err(|e| {
                vec![Failure {
                    entry: "(the whole list)".into(),
                    reason: e.to_string(),
                }]
            })
            .and_then(|loaded| crate::writer::apply::apply(&loaded, list));
        match result {
            Ok(applied) => {
                staged
                    .reports
                    .push(json!({"part": part, "name": name, "report": applied.to_dict()}));
                staged.unchanged_entries.extend(
                    applied
                        .outcome
                        .unchanged
                        .iter()
                        .map(|e| format!("{label}: {e}")),
                );
                staged.unchanged &= applied.unchanged;
                if !applied.unchanged {
                    staged.bytes = Arc::new(applied.bytes);
                    step = Arc::new(
                        part::File::of_shared(step.path(), Arc::clone(&staged.bytes))
                            .map_err(|e| SaveError::Failed(e.to_string()))?,
                    );
                }
            }
            Err(refused) => failures.extend(
                refused
                    .into_iter()
                    .map(|f| PartFailure::new(part, &label, f)),
            ),
        }
    }
    if failures.is_empty() {
        Ok(staged)
    } else {
        Err(SaveError::PartEdits(failures))
    }
}

/// Applies a list per part through the assembly writer, then commits the file once. The
/// session and disk are untouched on refusal.
pub fn save_assembly_edits(
    shared: &Shared,
    rev: Option<u64>,
    lists: &BTreeMap<usize, EditList>,
    source: &'static str,
) -> Result<Saved, SaveError> {
    save_with(shared, rev, source, |step, drafts| {
        let lists = include_drafts(lists.clone(), drafts, &step, source)?;
        stage_assembly_edits(step, &lists).map(StagedAssembly::save)
    })
}

/// The single save transaction for either request form, including the revision checks and
/// publication of the saved file and its new session state.
fn save_with(
    shared: &Shared,
    rev: Option<u64>,
    source: &'static str,
    apply: impl FnOnce(
        Arc<part::File>,
        &BTreeMap<usize, EditList>,
    ) -> Result<(Arc<Vec<u8>>, Value, bool), SaveError>,
) -> Result<Saved, SaveError> {
    let save_lock = Arc::clone(&lock(shared).save_lock);
    let _one_at_a_time = save_lock.lock().unwrap_or_else(|p| p.into_inner());
    let (seen, step, file, in_place, gz, mut backup, copy, drafts) = {
        let s = lock(shared);
        if rev.is_some_and(|r| r != s.rev) {
            return Err(SaveError::Stale(s.rev));
        }
        s.check_allowed()?;
        if s.in_place {
            let now = fs::read(&s.file.path)
                .map(|b| sha256(&b))
                .map_err(|e| SaveError::Failed(format!("{}: {e}", s.file.path.display())))?;
            if !s.is_ours(&now) {
                return Err(SaveError::Changed(format!(
                    "{} has changed on disk since it was opened: nothing is written over it",
                    s.file.path.display()
                )));
            }
        }
        (
            s.rev,
            Arc::clone(&s.step),
            s.file.clone(),
            s.in_place,
            s.gz,
            s.backup.clone(),
            s.own_copy().map(Path::to_path_buf),
            s.drafts(),
        )
    };
    let (text, mut report, unchanged) = apply(step, &drafts)?;
    // The file saved, as the session works on it from now on.
    let saved_step = part::File::of_shared(&file.path, Arc::clone(&text))
        .map_err(|e| SaveError::Failed(e.to_string()))?;
    let resumed = Session::of(file.clone(), saved_step, in_place);
    let mut analyses = BTreeMap::new();
    let mut answers = BTreeMap::new();
    // The required questions the saved file still leaves open, as `write` would list them.
    let mut missing = Vec::new();
    for k in 0..resumed.part_count() {
        let analysis =
            Arc::new(analyse(&resumed.step, k).map_err(|e| SaveError::Failed(e.to_string()))?);
        let resumed_answers = analysis["resume"]["answers"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Err(Error::Rules(crate::rules::Error::Incomplete(e))) =
            api::apply(&analysis, &resumed_answers, false)
        {
            missing.extend(e.missing);
        }
        if analysis["resume"]["answers"].is_object() {
            answers.insert(k, resumed_answers);
        }
        analyses.insert(k, analysis);
    }
    let hook = lock(shared).between_locks;
    if let Some(hook) = hook {
        hook(shared);
    }
    // An agent can change the answers while we stage. Keep the lock from this final
    // revision check through the write and the publication of the new session state.
    let mut s = lock(shared);
    if s.rev != seen {
        return Err(SaveError::Stale(s.rev));
    }
    if let Some(hook) = s.commit_boundary {
        hook(shared);
    }
    // Keep the session locked for the disk I/O, including the first .bak copy. An answer
    // accepted after this revision check would otherwise be overwritten by publication.
    let cannot = |e: io::Error| SaveError::Failed(format!("cannot save: {e}"));
    let (path, bytes, stored, sha) = if in_place {
        let stored = replace_input(&mut s, &file.path, gz, &mut backup, text.as_ref().clone())
            .map_err(cannot)?;
        let sha = sha256(&stored);
        (file.path.clone(), stored.len(), Some(stored), sha)
    } else {
        let bytes = text.len();
        let sha = sha256(&text);
        let path = match copy {
            Some(copy) => update_copy(&s, &file, copy, &text),
            None => write_new(&file, &text),
        };
        (path.map_err(cannot)?, bytes, None, sha)
    };
    let shown = path.display().to_string();
    report["output"] = json!(shown);
    if let Some(stored) = stored {
        s.backup = backup;
        s.current = Arc::new(stored);
    }
    s.step = resumed.step;
    s.analyses = analyses;
    let at = s.rev + 1;
    // Set the saved baseline before publishing answers, so saved decisions are no longer drafts.
    s.saved = answers.clone();
    // Each part once: one change (and event) per part, to the answers the saved file stores.
    let parts: std::collections::BTreeSet<usize> =
        s.answers.keys().chain(answers.keys()).copied().collect();
    for k in &parts {
        let now = answers.remove(k).unwrap_or_default();
        s.change(*k, now, source, Some(at));
    }
    // A save that changed the file is a new rev even when the stored answers are the same (a
    // fill writes PMI, not an answer): a page or agent holding the old rev must read again
    // before it saves. Each part's answers are sent again at it.
    if !unchanged && s.rev < at {
        s.rev = at;
        for k in parts {
            let answers = s.answers.get(&k).cloned().unwrap_or_default();
            s.changes.push_back(Change {
                rev: at,
                part: k,
                source,
                set: Map::new(),
                cleared: Vec::new(),
            });
            let data =
                json!({"rev": at, "part": k, "answers": answers, "source": source, "drafts": {}});
            s.publish(&event("answers", Some(at), &data));
        }
        while s.changes.len() > KEPT_CHANGES {
            if let Some(gone) = s.changes.pop_front() {
                s.dropped = gone.rev;
            }
        }
    }
    s.last_saved = Some((path, sha));
    let saved = Saved {
        path: shown,
        bytes,
        rev: s.rev,
        report,
        parts: s.parts(),
        complete: missing.is_empty(),
        missing,
    };
    let data = json!({"path": saved.path, "bytes": saved.bytes, "rev": saved.rev,
                      "source": source});
    s.publish(&event("saved", None, &data));
    Ok(saved)
}

/// `text` (an edit list, or a 0.2 answers file) checked against the session's file as its last
/// save left it, as [`save_edits`] would apply it (`writer::apply::check`). Nothing is written
/// and the session is not changed.
pub fn check_edits(shared: &Shared, text: &str) -> Outcome {
    check_part_edits(shared, None, text).1.outcome
}

pub(crate) struct CheckedEdits {
    outcome: Outcome,
    part_failures: Vec<PartFailure>,
}

impl CheckedEdits {
    fn plain(outcome: Outcome) -> Self {
        Self {
            outcome,
            part_failures: Vec::new(),
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        let mut data = self.outcome.to_json();
        if !self.part_failures.is_empty() {
            data["failures"] = json!(self.part_failures);
        }
        data
    }
}

/// Checks the opened part's list with working answers, and reports the revision checked.
pub(crate) fn check_part_edits(
    shared: &Shared,
    part: Option<usize>,
    text: &str,
) -> (u64, CheckedEdits) {
    let (rev, step, drafts) = {
        let s = lock(shared);
        (s.rev, Arc::clone(&s.step), s.drafts())
    };
    let loaded = match step.load(part) {
        Ok(loaded) => loaded,
        Err(e) => {
            return (
                rev,
                CheckedEdits::plain(Outcome::malformed(vec![Failure {
                    entry: "(the whole list)".into(),
                    reason: e.to_string(),
                }])),
            );
        }
    };
    if drafts.is_empty() {
        return (
            rev,
            CheckedEdits::plain(crate::writer::apply::check(&loaded, text)),
        );
    }
    let list = match edits::read(text) {
        Ok(list) => list,
        Err(failures) => return (rev, CheckedEdits::plain(Outcome::malformed(failures))),
    };
    let index = loaded.binding.part;
    let lists = BTreeMap::from([(index, list)]);
    let checked = check_with_drafts(step, lists, &drafts, Some(&loaded));
    (rev, checked)
}

/// Checks an assembly batch by staging the same writes as a save, with no disk effects.
pub(crate) fn check_assembly_edits(
    shared: &Shared,
    lists: &BTreeMap<usize, EditList>,
) -> (u64, CheckedEdits) {
    let (rev, step, drafts) = {
        let s = lock(shared);
        (s.rev, Arc::clone(&s.step), s.drafts())
    };
    (rev, check_with_drafts(step, lists.clone(), &drafts, None))
}

fn check_with_drafts(
    step: Arc<part::File>,
    lists: BTreeMap<usize, EditList>,
    drafts: &BTreeMap<usize, EditList>,
    loaded: Option<&part::LoadedPart>,
) -> CheckedEdits {
    let mut outcome = Outcome {
        well_formed: true,
        checked_against_file: false,
        applicable: true,
        entries: lists.values().map(EditList::len).sum(),
        unchanged: Vec::new(),
        failures: Vec::new(),
    };
    let result = include_drafts(lists, drafts, &step, "mcp").and_then(|lists| {
        outcome.checked_against_file = true;
        outcome.entries = lists.values().map(EditList::len).sum();
        if let Some(loaded) = loaded.filter(|_| lists.len() == 1) {
            crate::writer::apply::apply(loaded, &lists[&loaded.binding.part])
                .map(|applied| applied.outcome.unchanged)
                .map_err(SaveError::Edits)
        } else {
            stage_assembly_edits(step, &lists).map(|staged| staged.unchanged_entries)
        }
    });
    let mut part_failures = Vec::new();
    match result {
        Ok(unchanged) => outcome.unchanged = unchanged,
        Err(SaveError::Edits(failures)) => outcome.failures = failures,
        Err(SaveError::PartEdits(failures)) => {
            outcome.failures = failures.iter().map(PartFailure::plain).collect();
            part_failures = failures;
        }
        Err(e) => outcome.failures.push(Failure {
            entry: "(the whole list)".into(),
            reason: e.to_string(),
        }),
    }
    outcome.applicable = outcome.failures.is_empty();
    CheckedEdits {
        outcome,
        part_failures,
    }
}

/// An edit list's refusal in plain words: nothing was written, and each failing entry.
pub fn edits_refused(failures: &[crate::edits::Failure]) -> String {
    let mut message = "nothing was written: an edit list is applied all or nothing, and these \
                       entries cannot be applied:"
        .to_string();
    for f in failures {
        message.push_str(&format!("\n- {}: {}", f.entry, f.reason));
    }
    message
}

/// A later save of the session's own copy (see [`Session::own_copy`]): a temporary file is
/// written, then, under the session's lock, the copy is checked again and the file renamed onto
/// it. A copy no longer the session's by then is left alone: the save takes a new name.
fn update_copy(s: &Session, file: &Opened, copy: PathBuf, body: &[u8]) -> io::Result<PathBuf> {
    let dir = copy
        .parent()
        .ok_or_else(|| io::Error::other("the copy has no directory"))?;
    let temp = temp_file(dir, body)?;
    if s.own_copy() == Some(copy.as_path()) {
        let renamed = fs::rename(&temp, &copy);
        return match renamed {
            Ok(()) => Ok(copy),
            Err(e) => {
                let _ = fs::remove_file(&temp);
                Err(e)
            }
        };
    }
    let _ = fs::remove_file(&temp);
    write_new(file, body)
}

/// A save beside the input under the first free candidate name, taken with `create_new`
/// (nothing is overwritten, even in a race), the written temporary file renamed onto it.
fn write_new(file: &Opened, body: &[u8]) -> io::Result<PathBuf> {
    let dir = file
        .path
        .parent()
        .ok_or_else(|| io::Error::other("the input has no directory"))?;
    let stem = stem(&file.name);
    let temp = temp_file(dir, body)?;
    for n in 1..=MAX_CANDIDATES {
        let target = candidate(dir, stem, n);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(_) => {
                if let Err(e) = fs::rename(&temp, &target) {
                    let _ = fs::remove_file(&temp);
                    let _ = fs::remove_file(&target);
                    return Err(e);
                }
                return Ok(target);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                let _ = fs::remove_file(&temp);
                return Err(e);
            }
        }
    }
    let _ = fs::remove_file(&temp);
    Err(io::Error::other("every candidate name is taken"))
}

/// In place: the input `step` replaced by renaming a written temporary file onto it. The first
/// save of the session (`backup` None) first copies the original to the first free `.bak`
/// beside the path as given; later saves keep no other copy. A link is followed (its target
/// replaced, the link kept). The replacement keeps the original's permissions, and a gzipped
/// input is written gzipped. With the session locked, record the bytes as the session's own
/// once the rename succeeds. Returns the bytes stored.
fn replace_input(
    s: &mut Session,
    step: &Path,
    gz: bool,
    backup: &mut Option<PathBuf>,
    text: Vec<u8>,
) -> io::Result<Vec<u8>> {
    let real = fs::canonicalize(step)?;
    let dir = real
        .parent()
        .ok_or_else(|| io::Error::other("the input has no directory"))?;
    let stored = if gz {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&text)?;
        gz.finish()?
    } else {
        text
    };
    let temp = temp_file(dir, &stored)?;
    let replaced = (|| {
        fs::set_permissions(&temp, fs::metadata(&real)?.permissions())?;
        if backup.is_none() {
            *backup = Some(back_up(step, &real)?);
        }
        let sha = sha256(&stored);
        fs::rename(&temp, &real)?;
        if !s.written.contains(&sha) {
            s.written.push(sha);
        }
        Ok(())
    })();
    if let Err(e) = replaced {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    Ok(stored)
}

pub use crate::api::{Approved, approvals};

/// [`api::with_approved`], refused as a save is: an approval as [`SaveError::Question`].
/// `save`, and MCP's `write_pmi`.
pub fn with_approved(
    analysis: &Value,
    answers: &Map<String, Value>,
    part: usize,
    approved: &Map<String, Value>,
) -> Result<Map<String, Value>, SaveError> {
    api::with_approved(analysis, answers, part, approved).map_err(|e| match e {
        Error::Unapproved(u) => SaveError::Question {
            message: u.message,
            part: u.part,
            question: u.question,
            values: u.values,
        },
        other => SaveError::Refused(other.to_string()),
    })
}

/// A report without its PMI scenes (`report.pmi.scenes`, and each part's in an assembly).
fn without_scenes(mut value: Value) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(map) => {
                if let Some(Value::Object(pmi)) = map.get_mut("pmi") {
                    pmi.remove("scenes");
                }
                map.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut value);
    value
}

/// A server-sent event.
pub fn event(name: &str, id: Option<u64>, data: &Value) -> String {
    match id {
        Some(id) => format!("event: {name}\nid: {id}\ndata: {data}\n\n"),
        None => format!("event: {name}\ndata: {data}\n\n"),
    }
}

/// The input's name without `.gz` and `.step`/`.stp`.
fn stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    let mut end = name.len();
    if lower.ends_with(".gz") {
        end -= 3;
    }
    let base = &lower[..end];
    if base.ends_with(".step") {
        end -= 5;
    } else if base.ends_with(".stp") {
        end -= 4;
    }
    &name[..end]
}

/// The `n`th name a save may take: `<stem>.specified.step`, then `<stem>.specified-2.step`, …
fn candidate(dir: &Path, stem: &str, n: u32) -> PathBuf {
    if n == 1 {
        dir.join(format!("{stem}.specified.step"))
    } else {
        dir.join(format!("{stem}.specified-{n}.step"))
    }
}

/// The highest suffix tried before a save gives up.
const MAX_CANDIDATES: u32 = 10_000;

/// `body` written to a new temporary file in `dir`, synced.
fn temp_file(dir: &Path, body: &[u8]) -> io::Result<PathBuf> {
    let tag = crate::open::token().map_err(|e| io::Error::other(e.to_string()))?;
    let path = dir.join(format!(".specify-save-{}.tmp", &tag[..12]));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let written = file.write_all(body).and_then(|_| file.sync_all());
    if let Err(e) = written {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

/// `real` (the input's file) copied to the first free name of `<step>.bak`, `<step>.bak-2`, …
/// (never over a file); the name taken.
fn back_up(step: &Path, real: &Path) -> io::Result<PathBuf> {
    for n in 1..=MAX_CANDIDATES {
        let mut name = step.as_os_str().to_os_string();
        if n == 1 {
            name.push(".bak");
        } else {
            name.push(format!(".bak-{n}"));
        }
        let bak = PathBuf::from(name);
        match OpenOptions::new().write(true).create_new(true).open(&bak) {
            Ok(mut out) => {
                let copied = File::open(real)
                    .and_then(|mut input| io::copy(&mut input, &mut out))
                    .and_then(|_| out.sync_all());
                if let Err(e) = copied {
                    let _ = fs::remove_file(&bak);
                    return Err(e);
                }
                return Ok(bak);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("every .bak name is taken"))
}
