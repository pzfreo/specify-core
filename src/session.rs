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
//! are the ones the file stores (decided none, and choices); answers changed in the session
//! and not saved are never written, and a save is refused while there are any.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use serde_json::{Map, Value, json};
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
    /// The file could not be written.
    Failed(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Stale(rev) => write!(f, "the answers have moved on to rev {rev}"),
            SaveError::Question { message, .. } => f.write_str(message),
            SaveError::Edits(failures) => f.write_str(&edits_refused(failures)),
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
    /// One save at a time (held while writing, without the session's lock).
    save_lock: Arc<Mutex<()>>,
    /// Each part's answers as the file last saved (or opened) holds them: answers that differ
    /// are unsaved.
    saved: BTreeMap<usize, Map<String, Value>>,
    /// Tests only: called by a save after it applied its list, before it checks the session
    /// again and writes.
    #[doc(hidden)]
    pub between_locks: Option<fn(&Shared)>,
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

    /// Whether any part's answers differ from what the file last saved (or opened) holds.
    fn has_unsaved(&self) -> bool {
        let empty = Map::new();
        self.answers
            .keys()
            .chain(self.saved.keys())
            .any(|k| self.answers.get(k).unwrap_or(&empty) != self.saved.get(k).unwrap_or(&empty))
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
        let data = json!({"rev": self.rev, "parts": self.parts(), "source": "start"});
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

/// Applies the edit list `list` (ADR P11: all or nothing, touching only what it names) to the
/// session's file as its last save left it (the input before any), and saves the result:
/// in place, to the session's own copy, or beside the input under the first free
/// `<stem>.specified.step`. `rev` (None: any) is the session revision the caller last saw: a
/// session that has moved on is refused as [`SaveError::Stale`]. A list that cannot be applied
/// writes nothing ([`SaveError::Edits`], every failing entry named); so does a session holding
/// answers not saved (they would be lost). The session then works on the file saved: its
/// analyses are made again, and each part's answers are those the saved file resumes (a new
/// rev). Every subscriber gets a `saved` event.
pub fn save_edits(
    shared: &Shared,
    rev: Option<u64>,
    list: &crate::edits::EditList,
    source: &'static str,
) -> Result<Saved, SaveError> {
    let save_lock = Arc::clone(&lock(shared).save_lock);
    let _one_at_a_time = save_lock.lock().unwrap_or_else(|p| p.into_inner());
    let (seen, step, file, in_place, gz, mut backup, copy) = {
        let s = lock(shared);
        if rev.is_some_and(|r| r != s.rev) {
            return Err(SaveError::Stale(s.rev));
        }
        s.check_allowed()?;
        // The edits are applied to the file and the session then reads it back: answers not
        // yet saved would be lost, so they are saved or discarded first.
        if s.has_unsaved() {
            return Err(SaveError::Refused(UNSAVED.into()));
        }
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
        )
    };
    let loaded = step
        .load(None)
        .map_err(|e| SaveError::Refused(e.to_string()))?;
    let applied = crate::writer::apply::apply(&loaded, list).map_err(SaveError::Edits)?;
    // The file saved, as the session works on it from now on.
    let saved_step = part::File::of_shared(&file.path, Arc::new(applied.bytes.clone()))
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
    // Answers changed in the session while the list was applied (an agent's, which no rev
    // guards) would be replaced by the saved file's: refused, nothing written.
    {
        let s = lock(shared);
        if s.rev != seen {
            return Err(SaveError::Stale(s.rev));
        }
    }
    let cannot = |e: io::Error| SaveError::Failed(format!("cannot save: {e}"));
    let text = applied.bytes.clone();
    let (path, bytes, stored, sha) = if in_place {
        let stored = replace_input(shared, &file.path, gz, &mut backup, text).map_err(cannot)?;
        let sha = sha256(&stored);
        (file.path.clone(), stored.len(), Some(stored), sha)
    } else {
        let bytes = text.len();
        let sha = sha256(&text);
        let path = match copy {
            Some(copy) => update_copy(shared, &file, copy, &text),
            None => write_new(&file, &text),
        };
        (path.map_err(cannot)?, bytes, None, sha)
    };
    let shown = path.display().to_string();
    let mut report = applied.to_dict();
    report["output"] = json!(shown);
    let mut s = lock(shared);
    if let Some(stored) = stored {
        s.backup = backup;
        s.current = Arc::new(stored);
    }
    s.step = resumed.step;
    s.analyses = analyses;
    let at = s.rev + 1;
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
    if !applied.unchanged && s.rev < at {
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
            let data = json!({"rev": at, "part": k, "answers": answers, "source": source});
            s.publish(&event("answers", Some(at), &data));
        }
        while s.changes.len() > KEPT_CHANGES {
            if let Some(gone) = s.changes.pop_front() {
                s.dropped = gone.rev;
            }
        }
    }
    // Saved: nothing the session holds is unwritten.
    s.saved = s.answers.clone();
    s.last_saved = Some((path, sha));
    let saved = Saved {
        path: shown,
        bytes,
        rev: s.rev,
        report,
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
pub fn check_edits(shared: &Shared, text: &str) -> crate::edits::Outcome {
    let (step, unsaved) = {
        let s = lock(shared);
        (Arc::clone(&s.step), s.has_unsaved())
    };
    // As `save_edits` refuses it: the answers not saved would be lost.
    if unsaved {
        let mut outcome = crate::edits::check(text, None);
        outcome.failures.push(crate::edits::Failure {
            entry: "(the session)".into(),
            reason: UNSAVED.into(),
        });
        outcome.applicable = false;
        return outcome;
    }
    match step.load(None) {
        Ok(loaded) => crate::writer::apply::check(&loaded, text),
        Err(e) => crate::edits::Outcome::malformed(vec![crate::edits::Failure {
            entry: "(the whole list)".into(),
            reason: e.to_string(),
        }]),
    }
}

/// Why an edit list is refused while the session holds answers not saved.
const UNSAVED: &str = "the session has unsaved answers: save or discard the unsaved answers first; \
                       nothing was written";

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
fn update_copy(shared: &Shared, file: &Opened, copy: PathBuf, body: &[u8]) -> io::Result<PathBuf> {
    let dir = copy
        .parent()
        .ok_or_else(|| io::Error::other("the copy has no directory"))?;
    let temp = temp_file(dir, body)?;
    let s = lock(shared);
    if s.own_copy() == Some(copy.as_path()) {
        let renamed = fs::rename(&temp, &copy);
        drop(s);
        return match renamed {
            Ok(()) => Ok(copy),
            Err(e) => {
                let _ = fs::remove_file(&temp);
                Err(e)
            }
        };
    }
    drop(s);
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
/// input is written gzipped. What is stored is recorded as the session's own before the
/// rename (and forgotten if it fails), so a reopen or a check meanwhile never takes the file as
/// changed by someone else. Returns the bytes stored.
fn replace_input(
    shared: &Shared,
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
        let recorded = {
            let mut s = lock(shared);
            let new = !s.written.contains(&sha);
            if new {
                s.written.push(sha.clone());
            }
            new
        };
        let renamed = fs::rename(&temp, &real);
        if renamed.is_err() && recorded {
            lock(shared).written.retain(|w| *w != sha);
        }
        renamed
    })();
    if let Err(e) = replaced {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    Ok(stored)
}

pub use crate::api::{Approved, approvals};

/// [`api::with_approved`], refused as a save is: an approval as [`SaveError::Question`].
/// `save`, and MCP's `write_pmi` and `export_answers`.
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
