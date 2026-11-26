//! The Specify page server (`specify-core open`, and MCP's `show`): one session's STEP file
//! opened in the Specify app, served on 127.0.0.1 to the person's own browser, never uploaded,
//! and saved back to disk by this process, applying the edit list the page sends
//! (src/session.rs, src/edits.rs).
//!
//! Everything is under `http://127.0.0.1:<port>/<token>/` (the token 32 lowercase hex from the
//! operating system's random source), see docs/open.md:
//!
//! - `GET <token>/…` the static app (`index.html` at `<token>/`), `GET <token>/wasm/…` the
//!   engine, `GET <token>/file` the STEP bytes (`?rev=old` OLD's in a diff, `?rev=saved` what
//!   the last save wrote), `GET <token>/config` the app's settings, `GET <token>/diff` a
//!   diff's report;
//! - `GET <token>/answers` the session's answers, `GET <token>/events` their changes as
//!   server-sent events;
//! - `POST <token>/save` an edit list applied to the file, all or nothing, and written next to
//!   the input (never over a file, unless in place, which keeps the original once as `.bak`);
//! - `POST <token>/discard` drops working answers without writing;
//! - `POST <token>/closed` the page went away: the server stops [`CLOSE_DELAY`] after the last
//!   request unless another arrives (a reload, or another open page's heartbeat).
//!
//! An open page sends `GET <token>/config` every [`HEARTBEAT`]; any request with the address and
//! the token resets the idle time and cancels a pending close (one without either does not), and
//! an open event stream keeps the server running.
//!
//! A request whose `Host` is not exactly `127.0.0.1:<port>` is refused (a page that finds the
//! port through DNS rebinding), as is any other token, without saying why. `save` and
//! `closed` must carry `Origin: http://127.0.0.1:<port>`; a `GET` with any other
//! `Origin` is refused. A forgotten server stops after [`IDLE`] without a request.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::session;

mod bundle;

/// How often an open page asks for `config` (the contract's heartbeat; the server does not
/// use it, it sizes [`CLOSE_DELAY`]).
pub const HEARTBEAT: Duration = Duration::from_secs(30);

/// How long after `closed` the server stops, unless another request arrives: longer than
/// [`HEARTBEAT`], so another open page keeps it.
pub const CLOSE_DELAY: Duration = Duration::from_secs(90);

/// How long the server waits without a request before it stops (a forgotten server: an open
/// page's heartbeat keeps it).
pub const IDLE: Duration = Duration::from_secs(30 * 60);

/// The largest `save` body accepted (an edit list, as `input::JSON_LIMIT` takes JSON).
pub const MAX_SAVE_BODY: u64 = 16 * 1024 * 1024;

/// The largest `open` body accepted (a STEP file picked in the page, as `input::STEP_LIMIT`).
pub const MAX_OPEN_BODY: u64 = crate::input::STEP_LIMIT as u64;

/// The most event streams open at once; another is answered 503.
pub const MAX_STREAMS: usize = 4;

/// How often an event stream with nothing to say sends a comment.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The most connections answered at once; another is answered 503 and closed.
pub const MAX_CONNECTIONS: usize = 16;

/// The engine's entry point, relative to the app (`config.wasmUrl`).
pub const WASM_URL: &str = "wasm/specify_core_wasm.js";

/// The version of this page contract (docs/open.md), given as `config.api`. It changes only in
/// a minor release, when the contract changes in a way an app built for the old one cannot
/// follow; 1 is 0.2.0's contract, 2 is 0.3.0's (Save sends an edit list; no `PUT answers`).
pub const API_VERSION: u32 = 2;

/// The page's Content-Security-Policy: everything from this server only, the engine compiled
/// with `'wasm-unsafe-eval'`, inline styles (which UI libraries set), `data:` fonts and
/// `data:`/`blob:` images and downloads allowed.
pub const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; \
connect-src 'self' blob:; \
worker-src 'self' blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; \
form-action 'none'";

/// Why the file was not opened.
#[derive(Debug)]
pub enum Error {
    /// Refused before anything is served: a name that is not STEP, no app (exit 2).
    Refused(String),
    /// The file could not be read, the port opened or the token drawn.
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Refused(m) | Error::Io(m) => f.write_str(m),
        }
    }
}

/// Whether Specify opens a file of this name: `.step` or `.stp`, or either gzipped, in any
/// case.
pub fn is_step_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.strip_suffix(".gz").unwrap_or(&lower);
    base.ends_with(".step") || base.ends_with(".stp")
}

/// Compares two strings without stopping at the first difference (tokens).
pub(crate) fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// 16 bytes from the operating system's random source, as 32 lowercase hex characters (also
/// `specify-core mcp --http`'s bearer token).
pub(crate) fn token() -> Result<String, Error> {
    let mut bytes = [0u8; 16];
    random(&mut bytes).map_err(|e| Error::Io(format!("cannot draw a random token: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(unix)]
fn random(buf: &mut [u8]) -> io::Result<()> {
    fs::File::open("/dev/urandom")?.read_exact(buf)
}

#[cfg(windows)]
fn random(buf: &mut [u8]) -> io::Result<()> {
    // BCryptGenRandom with the system's preferred generator (BCRYPT_USE_SYSTEM_PREFERRED_RNG),
    // from bcrypt.dll, which every supported Windows has.
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(alg: *mut u8, buf: *mut u8, len: u32, flags: u32) -> i32;
    }
    let len = u32::try_from(buf.len()).map_err(io::Error::other)?;
    // SAFETY: a null algorithm handle is allowed with this flag; `buf` is valid for `len` bytes.
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), len, 2) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!("BCryptGenRandom: {status:#x}")))
    }
}

#[cfg(not(any(unix, windows)))]
fn random(_: &mut [u8]) -> io::Result<()> {
    Err(io::Error::other("no random source on this platform"))
}

/// Where the app and the engine are served from.
#[derive(Debug, Clone)]
pub struct Assets {
    /// The static app: `index.html` at its root.
    app: Root,
    /// The engine served at `wasm/`; without one, `wasm/` is the app's own.
    wasm: Option<Root>,
}

/// A directory, or files built into the command.
#[derive(Clone)]
enum Root {
    Dir(PathBuf),
    BuiltIn(bundle::Files),
}

impl std::fmt::Debug for Root {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Root::Dir(dir) => f.debug_tuple("Dir").field(dir).finish(),
            Root::BuiltIn(files) => write!(f, "BuiltIn({} files)", files.len()),
        }
    }
}

impl Root {
    /// The file at the request path `rel`, and its Content-Type.
    fn file(&self, rel: &str) -> Option<(Vec<u8>, &'static str)> {
        let segments = request_path(rel)?;
        match self {
            Root::Dir(dir) => static_file(dir, &segments),
            Root::BuiltIn(files) => {
                let path = segments.join("/");
                let bytes = bundle::file(files, &path)?;
                Some((bytes, content_type(Path::new(&path))))
            }
        }
    }
}

/// Resolve flag and environment overrides, or use the app and engine built into the command.
pub fn assets(app: Option<&Path>, wasm: Option<&Path>) -> Result<Assets, Error> {
    let from_env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let app = app
        .map(Path::to_path_buf)
        .or_else(|| from_env("SPECIFY_APP_DIR"));
    let wasm = wasm
        .map(Path::to_path_buf)
        .or_else(|| from_env("SPECIFY_WASM_DIR"));
    resolve_assets(app, wasm, bundle::app(), bundle::wasm())
}

/// `--app` (else the built-in app), and `--wasm` (else the app's own `wasm/` directory, else
/// the built-in engine).
fn resolve_assets(
    app: Option<PathBuf>,
    wasm: Option<PathBuf>,
    built_app: Option<bundle::Files>,
    built_wasm: Option<bundle::Files>,
) -> Result<Assets, Error> {
    let app = match app {
        Some(path) => {
            let dir = fs::canonicalize(&path)
                .ok()
                .filter(|d| d.join("index.html").is_file())
                .ok_or_else(|| {
                    app_error(format!(
                        "--app {}: not a directory holding index.html",
                        path.display()
                    ))
                })?;
            check_manifest(&dir)?;
            Root::Dir(dir)
        }
        None => {
            let files = built_app.ok_or_else(|| app_error(not_built_in("app")))?;
            let manifest = bundle::file(files, MANIFEST)
                .ok_or_else(|| app_error("the built-in app has no manifest"))?;
            check_manifest_bytes(&manifest, "the built-in app")?;
            Root::BuiltIn(files)
        }
    };
    let wasm = match wasm {
        Some(w) => Some(Root::Dir(
            fs::canonicalize(&w)
                .ok()
                .filter(|d| d.is_dir())
                .ok_or_else(|| {
                    Error::Refused(format!("--wasm {}: not a directory", w.display()))
                })?,
        )),
        None => match (&app, built_wasm) {
            (Root::Dir(dir), _) if dir.join("wasm").is_dir() => None,
            (_, Some(files)) => Some(Root::BuiltIn(files)),
            (Root::BuiltIn(_), None) => {
                return Err(Error::Refused(format!(
                    "{}: pass --wasm DIR / set SPECIFY_WASM_DIR",
                    not_built_in("WASM engine")
                )));
            }
            (Root::Dir(_), None) => None,
        },
    };
    Ok(Assets { app, wasm })
}

/// A build without the app or engine (made before `ui/` and the WASM were built).
fn not_built_in(what: &str) -> String {
    format!(
        "this specify-core was built without the Specify {what} (a release has it built in: \
         `pip install specify-core`; or build the WASM and ui/dist-app, then rebuild: see \
         docs/open.md)"
    )
}

fn app_error(message: impl std::fmt::Display) -> Error {
    Error::Refused(format!("{message}: pass --app DIR / set SPECIFY_APP_DIR"))
}

const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
const MANIFEST: &str = "specify-ui-app.json";

fn check_app_api(api: u32) -> Result<(), Error> {
    if api != API_VERSION {
        return Err(app_error(format!(
            "this app speaks session API {api}, specify-core {CORE_VERSION} speaks \
             {API_VERSION}: update the app, or run without --app to use the built-in one"
        )));
    }
    Ok(())
}

/// An `--app` with a manifest must speak this session API. Older local builds may omit it.
fn check_manifest(dir: &Path) -> Result<(), Error> {
    let path = dir.join(MANIFEST);
    match fs::read(&path) {
        Ok(bytes) => check_manifest_bytes(&bytes, &path.display().to_string()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let _ = writeln!(
                io::stderr(),
                "warning: {} is missing; continuing with the local app",
                path.display()
            );
            Ok(())
        }
        Err(e) => Err(app_error(format!(
            "cannot read app manifest {}: {e}",
            path.display()
        ))),
    }
}

/// The manifest names the app and its version, and speaks this session API. (build.rs builds
/// in only an app of this version.)
fn check_manifest_bytes(bytes: &[u8], at: &str) -> Result<(), Error> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        name: String,
        version: String,
        api: u32,
    }

    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|e| app_error(format!("invalid app manifest {at}: {e}")))?;
    check_app_api(manifest.api)?;
    if manifest.name != "specify-ui-app" || manifest.version.is_empty() {
        return Err(app_error(format!(
            "unexpected name or version in app manifest {at}"
        )));
    }
    Ok(())
}

/// How the page is served.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// No answer changes and no saves from the page (409).
    pub read_only: bool,
    /// The port on 127.0.0.1 (0: any free port).
    pub port: u16,
    /// `config.part`: the part the page is shown for (MCP's `show` of a document of a file of
    /// several parts); None for the whole file.
    pub part: Option<usize>,
}

/// The page server for one session, its port open and its app found, ready to serve.
pub struct App {
    listener: TcpListener,
    port: u16,
    token: String,
    shared: Shared,
}

/// A page server started in the background (MCP's `show`): its address and mode, whether it
/// is still running, and how to stop it.
pub struct Page {
    pub url: String,
    pub mode: &'static str,
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    unsaved: Arc<Mutex<Option<Instant>>>,
}

impl Page {
    /// Whether an open page said lately that it holds unsaved edits (`config?unsaved=1`), and the
    /// server still runs: not once its close delay has passed since it last did.
    pub fn holds_unsaved(&self) -> bool {
        self.is_running() && lock(&self.unsaved).is_some_and(|until| Instant::now() < until)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Stops the page server (its event streams end).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts a page server for `session` on a thread of its own (MCP's `show`): it stops as
/// [`App::serve`] does, or when the returned [`Page`] is stopped or dropped. Its notices go to
/// stderr (stdout may carry a protocol).
pub fn start(
    session: session::Shared,
    assets: Assets,
    options: &Options,
    close_delay: Duration,
    idle: Duration,
) -> Result<Page, Error> {
    let app = App::new(session, assets, options)?;
    let url = app.url();
    let mode = app.shared.mode;
    let stop = Arc::clone(&app.shared.stop);
    let unsaved = Arc::clone(&app.shared.unsaved);
    let running = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&running);
    thread::Builder::new()
        .spawn(move || {
            let _ = app.serve(close_delay, idle, |n| {
                let _ = writeln!(io::stderr(), "{n}");
            });
            flag.store(false, Ordering::SeqCst);
        })
        .map_err(|e| Error::Io(format!("cannot start the page server: {e}")))?;
    Ok(Page {
        url,
        mode,
        running,
        stop,
        unsaved,
    })
}

impl App {
    /// Opens a port on 127.0.0.1 for `session`'s page.
    pub fn new(session: session::Shared, assets: Assets, options: &Options) -> Result<App, Error> {
        let (diff, in_place, name) = {
            let s = session::lock(&session);
            (s.old().is_some(), s.in_place(), s.file().name.clone())
        };
        if options.read_only && in_place {
            return Err(Error::Refused(
                "--read-only and --in-place cannot be combined".to_string(),
            ));
        }
        if options.read_only && diff {
            return Err(Error::Refused(
                "--read-only and --diff cannot be combined".to_string(),
            ));
        }
        let mode = if options.read_only {
            "read-only"
        } else if diff {
            "diff"
        } else {
            "specify"
        };
        App::bind(name, mode, Some(session), None, assets, options)
    }

    /// Opens a port on 127.0.0.1 with no file yet: the page drops or chooses a STEP file, which
    /// `POST <token>/open` loads into a session whose saves write into `save_dir` (never over an
    /// existing file).
    pub fn empty(save_dir: PathBuf, assets: Assets, options: &Options) -> Result<App, Error> {
        let save_dir = std::path::absolute(&save_dir).map_err(|e| {
            Error::Io(format!(
                "cannot resolve save directory {}: {e}",
                save_dir.display()
            ))
        })?;
        let mode = if options.read_only {
            "read-only"
        } else {
            "specify"
        };
        App::bind(String::new(), mode, None, Some(save_dir), assets, options)
    }

    fn bind(
        name: String,
        mode: &'static str,
        session: Option<session::Shared>,
        save_dir: Option<PathBuf>,
        assets: Assets,
        options: &Options,
    ) -> Result<App, Error> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, options.port)).map_err(|e| {
            Error::Io(format!(
                "cannot open port {} on 127.0.0.1: {e}",
                options.port
            ))
        })?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::Io(e.to_string()))?
            .port();
        let token = token()?;
        let shared = Shared {
            host: format!("127.0.0.1:{port}"),
            origin: format!("http://127.0.0.1:{port}"),
            prefix: format!("/{token}/"),
            name,
            mode,
            session: Mutex::new(session),
            save_dir,
            assets,
            read_only: options.read_only,
            part: options.part,
            streams: AtomicUsize::new(0),
            stop: Arc::new(AtomicBool::new(false)),
            unsaved: Arc::default(),
            activity: Mutex::new(Activity {
                last: Instant::now(),
                accepted: 0,
                heard: 0,
                close_at: None,
            }),
        };
        Ok(App {
            listener,
            port,
            token,
            shared,
        })
    }

    /// The app's address, to open in the browser.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/{}/", self.port, self.token)
    }

    /// The file's name (`NEW`'s in a diff).
    pub fn name(&self) -> &str {
        &self.shared.name
    }

    /// The loopback port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Answers requests, each connection on its own thread (at most [`MAX_CONNECTIONS`] at
    /// once), until `close_delay` after `closed` with no request and no event stream since, or
    /// `idle` without a request (an open event stream keeps it running). `notice` gets each
    /// save's and the stop's one-line notice.
    pub fn serve(
        self,
        close_delay: Duration,
        idle: Duration,
        mut notice: impl FnMut(&str),
    ) -> Result<(), Error> {
        let port = self.port;
        let io = |e: io::Error| Error::Io(format!("serving on 127.0.0.1:{port}: {e}"));
        self.listener.set_nonblocking(true).map_err(io)?;
        let shared = Arc::new(self.shared);
        let active = Arc::new(AtomicUsize::new(0));
        let (tell, notices) = mpsc::channel::<String>();
        loop {
            while let Ok(n) = notices.try_recv() {
                notice(&n);
            }
            if shared.stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            if active.load(Ordering::SeqCst) == 0 {
                let now = Instant::now();
                let a = lock(&shared.activity);
                if a.close_at.is_some_and(|t| now >= t) {
                    notice("Closed.");
                    return Ok(());
                }
                if now.duration_since(a.last) >= idle {
                    notice(&format!(
                        "Stopped: no request for {} minutes.",
                        idle.as_secs() / 60
                    ));
                    return Ok(());
                }
            }
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let seq = {
                        let mut a = lock(&shared.activity);
                        a.accepted += 1;
                        a.accepted
                    };
                    let _ = stream.set_nonblocking(false);
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                        let _ = respond(
                            &mut stream,
                            "503 Service Unavailable",
                            &[("Retry-After", "1")],
                            b"",
                        );
                        continue;
                    }
                    let slot = Slot(active.clone());
                    let (shared, tell) = (shared.clone(), tell.clone());
                    // A thread that cannot be started drops the connection (and its slot),
                    // which the browser can retry.
                    let _ = thread::Builder::new().spawn(move || {
                        let _slot = slot;
                        let ctx = Ctx {
                            shared: &shared,
                            seq,
                            close_delay,
                            tell: &tell,
                        };
                        exchange(&mut stream, &ctx);
                        drain(&mut stream);
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(io(e)),
            }
        }
    }
}

/// What every connection shares.
struct Shared {
    host: String,
    origin: String,
    /// `/<token>/`.
    prefix: String,
    name: String,
    /// `config.mode`: `specify`, `read-only` or `diff`.
    mode: &'static str,
    /// None until a file is picked in an empty `specify-core open`.
    session: Mutex<Option<session::Shared>>,
    /// Where a file picked in the page saves (absolute); None when opened from a path on disk.
    save_dir: Option<PathBuf>,
    assets: Assets,
    read_only: bool,
    /// `config.part`.
    part: Option<usize>,
    /// Event streams open now.
    streams: AtomicUsize,
    /// Set to stop the server (and end its event streams).
    stop: Arc<AtomicBool>,
    /// Until when a page is taken to hold unsaved edits, from the last `config?unsaved=1`.
    unsaved: Arc<Mutex<Option<Instant>>>,
    activity: Mutex<Activity>,
}

impl Shared {
    fn session(&self) -> Option<session::Shared> {
        lock(&self.session).clone()
    }
}

/// When the server last heard from anyone, and whether it is closing.
struct Activity {
    last: Instant,
    /// Connections accepted so far.
    accepted: u64,
    /// The latest of them whose request carried the address and the token: `closed` stops the
    /// server only if none came after it.
    heard: u64,
    close_at: Option<Instant>,
}

struct Ctx<'a> {
    shared: &'a Shared,
    /// This connection's place in [`Activity::accepted`].
    seq: u64,
    close_delay: Duration,
    tell: &'a mpsc::Sender<String>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A served connection, counted until its thread ends.
pub(crate) struct Slot(pub(crate) Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The most read from a connection after its response before it is closed anyway.
const MAX_DRAIN: usize = 1024 * 1024;

/// After the response is sent and the write side shut: reads what the client still sends (a
/// body a refusal never read) until it closes, briefly, so closing does not reset the
/// connection and lose the response.
pub(crate) fn drain(stream: &mut TcpStream) {
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut sink = [0u8; 16 * 1024];
    let mut left = MAX_DRAIN;
    while left > 0 {
        let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if wait.is_zero() || stream.set_read_timeout(Some(wait)).is_err() {
            break;
        }
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => left = left.saturating_sub(n),
        }
    }
}

/// Headers on every answer from the app's address.
const COMMON: [(&str, &str); 4] = [
    ("Cache-Control", "no-store"),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("Content-Security-Policy", CSP),
];

fn reply(stream: &mut TcpStream, status: &str, content_type: Option<&str>, body: &[u8]) {
    let mut headers: Vec<(&str, &str)> = COMMON.to_vec();
    if let Some(t) = content_type {
        headers.push(("Content-Type", t));
    }
    let _ = respond(stream, status, &headers, body);
}

fn reply_json(stream: &mut TcpStream, status: &str, value: &serde_json::Value) {
    reply(
        stream,
        status,
        Some("application/json"),
        value.to_string().as_bytes(),
    );
}

fn refuse(stream: &mut TcpStream, status: &str, error: &str) {
    reply_json(stream, status, &json!({ "error": error }));
}

/// The value of `name` in a request target's query, if it has one.
fn query<'a>(target: &'a str, name: &str) -> Option<&'a str> {
    target
        .split_once('?')?
        .1
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

/// Answers one request.
fn exchange(stream: &mut TcpStream, ctx: &Ctx) {
    let shared = ctx.shared;
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let Some((request, body)) = read_head(stream) else {
        let _ = respond(stream, "400 Bad Request", &[], b"");
        return;
    };
    // This address only: a page that finds the port through DNS rebinding is refused.
    if request.header("host") != Some(shared.host.as_str()) {
        let _ = respond(stream, "403 Forbidden", &[], b"");
        return;
    }
    let path = request.target.split('?').next().unwrap_or("");
    // The token, exactly (compared in constant time); anything else is not found, without
    // saying more.
    let token = &shared.prefix[1..shared.prefix.len() - 1];
    let (first, rest) = match path.strip_prefix('/') {
        Some(p) => match p.split_once('/') {
            Some((first, rest)) => (first, Some(rest)),
            None => (p, None),
        },
        None => ("", None),
    };
    if !same(first, token) {
        let _ = respond(stream, "404 Not Found", &[], b"");
        return;
    }
    // Only a request with the address and the token counts as heard from: anything else that
    // finds the port neither keeps the server running nor cancels a close.
    {
        let mut a = lock(&shared.activity);
        a.last = Instant::now();
        // In the order they came, not the order their threads got here.
        if ctx.seq > a.heard {
            a.heard = ctx.seq;
            a.close_at = None;
        }
    }
    let Some(rest) = rest else {
        let location = shared.prefix.as_str();
        let _ = respond(
            stream,
            "308 Permanent Redirect",
            &[("Location", location)],
            b"",
        );
        return;
    };
    let origin = request.header("origin");
    let same_origin = origin == Some(shared.origin.as_str());
    let diff = shared.mode == "diff";
    match (request.method.as_str(), rest) {
        ("POST", "open") => {
            if !same_origin {
                return refuse(stream, "403 Forbidden", "open only from the app's own page");
            }
            open_file(stream, ctx, &request, body);
        }
        ("POST", "save") => {
            if !same_origin {
                return refuse(stream, "403 Forbidden", "save only from the app's own page");
            }
            save(stream, ctx, &request, body);
        }
        ("POST", "discard") => {
            if !same_origin {
                return refuse(
                    stream,
                    "403 Forbidden",
                    "discard only from the app's own page",
                );
            }
            discard(stream, ctx, &request, body);
        }
        ("POST", "closed") => {
            if !same_origin {
                return refuse(stream, "403 Forbidden", "only from the app's own page");
            }
            {
                let mut a = lock(&shared.activity);
                if a.heard == ctx.seq {
                    a.close_at = Some(Instant::now() + ctx.close_delay);
                }
            }
            reply(stream, "204 No Content", None, b"");
        }
        (_, "open" | "save" | "discard" | "closed") => {
            let _ = respond(stream, "405 Method Not Allowed", &[("Allow", "POST")], b"");
        }
        ("GET", _) if origin.is_some() && !same_origin => {
            let _ = respond(stream, "403 Forbidden", &[], b"");
        }
        ("GET", "answers") => {
            let Some(session) = shared.session() else {
                return refuse(stream, "404 Not Found", "no file is open yet");
            };
            let answers = session::lock(&session).answer_snapshot();
            reply_json(stream, "200 OK", &answers);
        }
        (_, "answers") => {
            let _ = respond(stream, "405 Method Not Allowed", &[("Allow", "GET")], b"");
        }
        ("GET", "file") => {
            let Some(session) = shared.session() else {
                return refuse(stream, "404 Not Found", "no file is open yet");
            };
            let bytes = match query(&request.target, "rev") {
                None | Some("new") => session::lock(&session).current(),
                Some("old") if diff => {
                    let s = session::lock(&session);
                    Arc::clone(&s.old().expect("diff mode has OLD").bytes)
                }
                Some("saved") => return saved_file(stream, &session),
                Some(_) => return reply(stream, "404 Not Found", None, b""),
            };
            reply(stream, "200 OK", Some("application/octet-stream"), &bytes);
        }
        ("GET", "diff") => {
            let report = shared
                .session()
                .and_then(|session| session::lock(&session).comparison().cloned());
            match report {
                Some(report) if diff => reply_json(stream, "200 OK", &report),
                _ => reply(stream, "404 Not Found", None, b""),
            }
        }
        ("GET", "events") => events(stream, ctx),
        ("GET", "config") => {
            // Not cleared by another tab that has none: a page with edits says so at each heartbeat.
            if query(&request.target, "unsaved") == Some("1") {
                *lock(&shared.unsaved) = Some(Instant::now() + ctx.close_delay);
            }
            let config = config_json(shared);
            reply_json(stream, "200 OK", &config);
        }
        ("GET", rest) => {
            let rest = if rest.is_empty() { "index.html" } else { rest };
            let (root, rel) = match (&shared.assets.wasm, rest.strip_prefix("wasm/")) {
                (Some(wasm), Some(rel)) => (wasm, rel),
                _ => (&shared.assets.app, rest),
            };
            match root.file(rel) {
                Some((bytes, content_type)) => reply(stream, "200 OK", Some(content_type), &bytes),
                None => reply(stream, "404 Not Found", None, b""),
            }
        }
        _ => {
            let _ = respond(stream, "405 Method Not Allowed", &[("Allow", "GET")], b"");
        }
    }
}

/// `GET <token>/config`'s body: the same shape before and after a file is picked in the page.
fn config_json(shared: &Shared) -> Value {
    let session = shared.session();
    let (name, path, old_name, old_path, target, saved_path) = match session {
        Some(session) => {
            let s = session::lock(&session);
            let target = if shared.read_only {
                None
            } else {
                s.save_target().map(|p| p.display().to_string())
            };
            let old = s.old();
            // A file picked in the browser has no known input path on disk; saves write into
            // `save_dir` under `<stem>.specified.step` (`saveTarget`).
            let path = if shared.save_dir.is_some() {
                None
            } else {
                Some(s.file().path.display().to_string())
            };
            (
                Some(s.file().name.clone()),
                path,
                old.map(|o| o.name.clone()),
                old.map(|o| o.path.display().to_string()),
                target,
                s.last_saved().map(|(p, _)| p.display().to_string()),
            )
        }
        None => (None, None, None, None, None, None),
    };
    json!({
        "api": API_VERSION,
        "mode": shared.mode,
        "name": name,
        "path": path,
        "part": shared.part,
        "oldName": old_name,
        "oldPath": old_path,
        "readOnly": shared.read_only,
        "wasmUrl": WASM_URL,
        "saveTarget": target,
        "savedPath": saved_path,
        "specifyCore": env!("CARGO_PKG_VERSION"),
    })
}

/// A percent-encoded plain filename (no path segments or control characters) that Specify opens.
fn plain_step_name(raw: &str) -> Option<String> {
    let name = percent_decode(raw)?;
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '\0'])
        || name.bytes().any(|b| b < 0x20 || b == 0x7f)
        || !is_step_name(&name)
    {
        return None;
    }
    Some(name)
}

/// Checks that an uploaded STEP body is non-empty and, if gzipped, decompresses cleanly within
/// [`MAX_OPEN_BODY`] (matching the browser viewer's `input::STEP_LIMIT`) before `Session::open`
/// allocates or parses it.
fn check_open_bytes(bytes: &[u8]) -> Result<(), (&'static str, String)> {
    if bytes.is_empty() {
        return Err(("400 Bad Request", "the STEP file is empty".into()));
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = flate2::read::MultiGzDecoder::new(bytes).take(MAX_OPEN_BODY + 1);
        let mut chunk = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            match decoder.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    total += n as u64;
                    if total > MAX_OPEN_BODY {
                        return Err((
                            "413 Content Too Large",
                            format!("uncompressed STEP is larger than {MAX_OPEN_BODY} bytes"),
                        ));
                    }
                }
                Err(e) => {
                    return Err((
                        "422 Unprocessable Content",
                        format!("the file is gzip but cannot be decompressed: {e}"),
                    ));
                }
            }
        }
        if total == 0 {
            return Err(("400 Bad Request", "the STEP file is empty".into()));
        }
    }
    Ok(())
}

/// `POST <token>/open`, from the app's own origin: loads a STEP file dropped or chosen in an
/// empty `specify-core open` page into a session whose saves write into `save_dir`.
fn open_file(stream: &mut TcpStream, ctx: &Ctx, request: &Request, body: Vec<u8>) {
    let shared = ctx.shared;
    let Some(save_dir) = shared.save_dir.as_ref() else {
        return refuse(stream, "409 Conflict", "a file is already open");
    };
    if shared.session().is_some() {
        return refuse(stream, "409 Conflict", "a file is already open");
    }
    let Some(raw_name) = request.header("x-specify-name") else {
        return refuse(
            stream,
            "400 Bad Request",
            "X-Specify-Name header is required",
        );
    };
    let Some(name) = plain_step_name(raw_name) else {
        return refuse(
            stream,
            "400 Bad Request",
            "X-Specify-Name must be a plain .step, .stp, .step.gz or .stp.gz filename",
        );
    };
    let bytes = match read_body(
        stream,
        request,
        body,
        MAX_OPEN_BODY,
        "application/octet-stream",
    ) {
        Ok(bytes) => bytes,
        Err((status, why)) => return refuse(stream, status, &why),
    };
    if let Err((status, why)) = check_open_bytes(&bytes) {
        return refuse(stream, status, &why);
    }
    let opened = session::Opened::of(save_dir.join(&name), bytes);
    let mut session = match session::Session::open(opened, false) {
        Ok(session) => session,
        Err(err) => return refuse(stream, "422 Unprocessable Content", &err.to_string()),
    };
    if session.parts().as_object().is_some_and(|p| p.is_empty())
        && let Err(err) = session.analysis(0)
    {
        return refuse(stream, "422 Unprocessable Content", &err.to_string());
    }
    {
        let mut slot = lock(&shared.session);
        if slot.is_some() {
            return refuse(stream, "409 Conflict", "a file is already open");
        }
        *slot = Some(Arc::new(Mutex::new(session)));
    }
    let _ = ctx.tell.send(format!("Opened {name}."));
    let config = config_json(shared);
    reply_json(stream, "200 OK", &config);
}

/// A JSON request body: `Content-Type: application/json` (415), a `Content-Length` and no
/// `Transfer-Encoding` (411), at most `max` bytes (413), all of it within 30 s and 1 s more per
/// MiB (408), parsed (400). The refusal's status and message otherwise.
fn json_body(
    stream: &mut TcpStream,
    request: &Request,
    body: Vec<u8>,
    max: u64,
) -> Result<serde_json::Value, (&'static str, String)> {
    let body = json_bytes(stream, request, body, max)?;
    serde_json::from_slice(&body).map_err(|e| ("400 Bad Request", format!("not JSON: {e}")))
}

/// [`json_body`]'s checks, the body's bytes as sent (to read it again keeping duplicate keys).
fn json_bytes(
    stream: &mut TcpStream,
    request: &Request,
    body: Vec<u8>,
    max: u64,
) -> Result<Vec<u8>, (&'static str, String)> {
    let body = read_body(stream, request, body, max, "application/json")?;
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|e| ("400 Bad Request", format!("not JSON: {e}")))?;
    Ok(body)
}

/// Reads a fixed-length request body of `expected_type`: `Content-Type` (415), `Content-Length`
/// with no `Transfer-Encoding` (411), at most `max` bytes (413), all within [`body_time`] (408).
fn read_body(
    stream: &mut TcpStream,
    request: &Request,
    mut body: Vec<u8>,
    max: u64,
    expected_type: &str,
) -> Result<Vec<u8>, (&'static str, String)> {
    let matches_type = request
        .header("content-type")
        .and_then(|t| t.split(';').next())
        .is_some_and(|t| t.trim().eq_ignore_ascii_case(expected_type));
    if !matches_type {
        return Err((
            "415 Unsupported Media Type",
            format!("send the body as {expected_type}"),
        ));
    }
    // The body is read by its Content-Length only: a chunked (or otherwise encoded) body is
    // refused rather than misread.
    let length = request
        .header("content-length")
        .and_then(|l| l.trim().parse::<u64>().ok());
    let (Some(length), None) = (length, request.header("transfer-encoding")) else {
        return Err((
            "411 Length Required",
            "Content-Length is required (no Transfer-Encoding)".into(),
        ));
    };
    if length > max {
        return Err(("413 Content Too Large", format!("larger than {max} bytes")));
    }
    let deadline = Instant::now() + body_time(length);
    let length = length as usize;
    body.truncate(length);
    let mut chunk = vec![0u8; 64 * 1024];
    while body.len() < length {
        let Some(left) = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
        else {
            return Err(("408 Request Timeout", "the body came too slowly".into()));
        };
        if stream
            .set_read_timeout(Some(left.min(Duration::from_secs(30))))
            .is_err()
        {
            return Err(("400 Bad Request", "the body cannot be read".into()));
        }
        match stream.read(&mut chunk[..(length - body.len()).min(64 * 1024)]) {
            Ok(0) | Err(_) => {
                return Err(("400 Bad Request", "the body ended early".into()));
            }
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    Ok(body)
}

/// The longest a body may take to arrive: 30 s, and 1 s more per MiB (a trickling client
/// cannot hold a connection indefinitely).
fn body_time(length: u64) -> Duration {
    Duration::from_secs(30 + length / (1024 * 1024))
}

/// `GET <token>/events`: the session's changes as server-sent events, the current state first;
/// a comment every [`KEEP_ALIVE`]. At most [`MAX_STREAMS`] at once.
fn events(stream: &mut TcpStream, ctx: &Ctx) {
    let shared = ctx.shared;
    let Some(session) = shared.session() else {
        return refuse(stream, "404 Not Found", "no file is open yet");
    };
    if shared.streams.fetch_add(1, Ordering::SeqCst) >= MAX_STREAMS {
        shared.streams.fetch_sub(1, Ordering::SeqCst);
        let mut headers: Vec<(&str, &str)> = COMMON.to_vec();
        headers.push(("Content-Type", "application/json"));
        headers.push(("Retry-After", "1"));
        let body = json!({"error": format!("at most {MAX_STREAMS} event streams at once")});
        let _ = respond(
            stream,
            "503 Service Unavailable",
            &headers,
            body.to_string().as_bytes(),
        );
        return;
    }
    let received = session::lock(&session).subscribe();
    let mut head = String::from("HTTP/1.1 200 OK\r\n");
    for (name, value) in COMMON {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Content-Type: text/event-stream\r\nConnection: close\r\n\r\n");
    let mut sent = stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.flush());
    let mut quiet_since = Instant::now();
    while sent.is_ok() && !shared.stop.load(Ordering::SeqCst) && !hung_up(stream) {
        match received.recv_timeout(Duration::from_millis(250)) {
            Ok(text) => {
                sent = stream
                    .write_all(text.as_bytes())
                    .and_then(|_| stream.flush());
                quiet_since = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if quiet_since.elapsed() >= KEEP_ALIVE {
                    sent = stream
                        .write_all(b": keep-alive\n\n")
                        .and_then(|_| stream.flush());
                    quiet_since = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
    shared.streams.fetch_sub(1, Ordering::SeqCst);
    // An open stream kept the server; its end counts as the last request.
    let mut a = lock(&shared.activity);
    a.last = Instant::now();
    if a.close_at.is_some() {
        a.close_at = Some(Instant::now() + ctx.close_delay);
    }
}

/// Whether the client has closed its side of an event stream (it sends nothing else).
fn hung_up(stream: &mut TcpStream) -> bool {
    if stream
        .set_read_timeout(Some(Duration::from_millis(1)))
        .is_err()
    {
        return true;
    }
    let mut byte = [0u8; 64];
    match stream.read(&mut byte) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => !matches!(
            e.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
        ),
    }
}

/// `GET <token>/file?rev=saved`: the bytes the session's last successful save wrote, read from
/// disk now and served only if they still hash to what that save stored; the file's absolute
/// path in `X-Specify-Path` (docs/open.md, `file`).
fn saved_file(stream: &mut TcpStream, session: &session::Shared) {
    let saved = session::lock(session)
        .last_saved()
        .map(|(path, sha)| (path.to_path_buf(), sha.to_string()));
    let Some((path, sha)) = saved else {
        return refuse(
            stream,
            "404 Not Found",
            "nothing has been saved in this session",
        );
    };
    let shown = path.display().to_string();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return reply_json(
                stream,
                "404 Not Found",
                &json!({"error": format!("{shown} no longer exists: it was removed after it was saved"),
                        "path": shown}),
            );
        }
        Err(e) => {
            return reply_json(
                stream,
                "500 Internal Server Error",
                &json!({"error": format!("cannot read {shown}: {e}"), "path": shown}),
            );
        }
    };
    if session::sha256(&bytes) != sha {
        return reply_json(
            stream,
            "409 Conflict",
            &json!({"error": format!("{shown} has changed on disk since it was saved"),
                    "path": shown}),
        );
    }
    let header = header_value(&shown);
    let mut headers: Vec<(&str, &str)> = COMMON.to_vec();
    headers.push(("Content-Type", "application/octet-stream"));
    headers.push(("X-Specify-Path", &header));
    let _ = respond(stream, "200 OK", &headers, &bytes);
}

/// `text` as a header value: printable ASCII as it is, `%` and every other byte of its UTF-8
/// percent-encoded (so `decodeURIComponent` gives `text` back, and a path of plain characters
/// is unchanged).
fn header_value(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if (0x20..0x7f).contains(&b) && b != b'%' {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The two API 2 request forms. The original edit list also accepts a 0.2 answers
/// map read as fills, as `write` reads one.
enum PageEdits {
    Single(Box<crate::edits::EditList>),
    Assembly(BTreeMap<usize, crate::edits::EditList>),
}

impl PageEdits {
    fn read(value: &Value) -> Result<Self, Value> {
        let bad = |why: &str| json!({"error": why});
        match (value.get("edits"), value.get("partEdits")) {
            (edits, None) => {
                let object = edits.filter(|e| e.is_object()).ok_or_else(|| {
                    bad("edits is required: the edit list to apply ({} for none)")
                })?;
                crate::edits::read(&object.to_string())
                    .map(|list| Self::Single(Box::new(list)))
                    .map_err(|failures| json!({"error": session::edits_refused(&failures), "failures": failures}))
            }
            (None, Some(parts)) => {
                session::read_part_edits(parts)
                    .map(Self::Assembly)
                    .map_err(|e| match e {
                        session::PartEditsError::Shape(why) => bad(&why),
                        session::PartEditsError::List(failures) => json!({
                            "error": session::part_edits_refused(&failures), "failures": failures
                        }),
                    })
            }
            (Some(_), Some(_)) => Err(bad("send either edits or partEdits")),
        }
    }
}

/// `POST <token>/save`, from the app's own origin: an edit list or a list per part,
/// applied all or nothing and written by the command (docs/open.md, `save`).
fn save(stream: &mut TcpStream, ctx: &Ctx, request: &Request, body: Vec<u8>) {
    let shared = ctx.shared;
    if shared.read_only {
        return refuse(stream, "409 Conflict", "opened read-only: nothing is saved");
    }
    let Some(session) = shared.session() else {
        return refuse(stream, "409 Conflict", "no file is open yet");
    };
    let value = match json_body(stream, request, body, MAX_SAVE_BODY) {
        Ok(v) => v,
        Err((status, why)) => return refuse(stream, status, &why),
    };
    let Some(rev) = value.get("rev").and_then(|v| v.as_u64()) else {
        return refuse(
            stream,
            "400 Bad Request",
            "rev is required: the session revision the page last saw",
        );
    };
    let result = match PageEdits::read(&value) {
        Ok(PageEdits::Single(list)) => session::save_edits(&session, Some(rev), &list, "page"),
        Ok(PageEdits::Assembly(lists)) => {
            session::save_assembly_edits(&session, Some(rev), &lists, "page")
        }
        Err(error) => return reply_json(stream, "400 Bad Request", &error),
    };
    match result {
        Ok(saved) => {
            let _ = ctx
                .tell
                .send(format!("Saved {} ({} bytes).", saved.path, saved.bytes));
            let mut response = saved.to_json();
            response["parts"] = saved.parts;
            reply_json(stream, "200 OK", &response);
        }
        Err(session::SaveError::Stale(current)) => reply_json(
            stream,
            "409 Conflict",
            &json!({"error": "the session has changed since that rev: show it and save again", "rev": current}),
        ),
        Err(session::SaveError::Changed(why)) => {
            let current = session::lock(&session).rev();
            reply_json(
                stream,
                "409 Conflict",
                &json!({"error": why, "rev": current}),
            )
        }
        Err(session::SaveError::Outside(why)) => refuse(stream, "403 Forbidden", &why),
        Err(session::SaveError::Edits(failures)) => reply_json(
            stream,
            "422 Unprocessable Content",
            &json!({"error": session::edits_refused(&failures), "failures": failures}),
        ),
        Err(session::SaveError::PartEdits(failures)) => reply_json(
            stream,
            "422 Unprocessable Content",
            &json!({"error": session::part_edits_refused(&failures), "failures": failures}),
        ),
        Err(e @ (session::SaveError::Refused(_) | session::SaveError::Question { .. })) => {
            refuse(stream, "422 Unprocessable Content", &e.to_string())
        }
        Err(session::SaveError::Failed(why)) => refuse(stream, "500 Internal Server Error", &why),
    }
}

/// A page drops all working answers or one working answer by part and question id.
fn discard(stream: &mut TcpStream, ctx: &Ctx, request: &Request, body: Vec<u8>) {
    let shared = ctx.shared;
    if shared.read_only {
        return refuse(
            stream,
            "409 Conflict",
            "opened read-only: nothing is discarded",
        );
    }
    let Some(session) = shared.session() else {
        return refuse(stream, "409 Conflict", "no file is open yet");
    };
    let value = match json_body(stream, request, body, MAX_SAVE_BODY) {
        Ok(v) => v,
        Err((status, why)) => return refuse(stream, status, &why),
    };
    let Some(rev) = value.get("rev").and_then(Value::as_u64) else {
        return refuse(stream, "400 Bad Request", "rev is required");
    };
    let target = match (value.get("part"), value.get("id")) {
        (None, None) => None,
        (Some(part), Some(id)) => {
            let part = part.as_u64().and_then(|n| usize::try_from(n).ok());
            let id = id.as_str().filter(|id| !id.is_empty());
            match part.zip(id) {
                Some(target) => Some(target),
                None => {
                    return refuse(
                        stream,
                        "400 Bad Request",
                        "part and id must identify a working answer",
                    );
                }
            }
        }
        _ => {
            return refuse(
                stream,
                "400 Bad Request",
                "send both part and id to discard one working answer",
            );
        }
    };
    match session::discard_answers(&session, Some(rev), target, "page") {
        Ok(snapshot) => reply_json(stream, "200 OK", &snapshot),
        Err(session::SaveError::Stale(current)) => reply_json(
            stream,
            "409 Conflict",
            &json!({"error": "the session changed: review the latest answers before discarding", "rev": current}),
        ),
        Err(err) => refuse(stream, "400 Bad Request", &err.to_string()),
    }
}

/// The segments of the request path `rel`, decoded; None for an encoded or literal `..`, `.`,
/// an empty segment, a backslash or colon.
fn request_path(rel: &str) -> Option<Vec<String>> {
    rel.split('/')
        .map(|raw| {
            percent_decode(raw).filter(|segment| {
                !(segment.is_empty()
                    || segment == "."
                    || segment == ".."
                    || segment.contains(['/', '\\', ':', '\0']))
            })
        })
        .collect()
}

/// A file inside `root` (canonical) at `segments`, and its Content-Type; None for a directory,
/// or a link that leads outside.
fn static_file(root: &Path, segments: &[String]) -> Option<(Vec<u8>, &'static str)> {
    let mut path = root.to_path_buf();
    path.extend(segments);
    let real = fs::canonicalize(&path).ok()?;
    if !real.starts_with(root) || !real.is_file() {
        return None;
    }
    let bytes = fs::read(&real).ok()?;
    Some((bytes, content_type(&real)))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The Content-Type a file is served with, by its extension.
pub fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// A request's method, target and headers, as far as they are read.
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) headers: Vec<(String, String)>,
}

impl Request {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Reads a request's head (up to 16 KiB, all of it within 10 s), and the bytes read past it
/// (the start of a body); None for anything else.
pub(crate) fn read_head(stream: &mut TcpStream) -> Option<(Request, Vec<u8>)> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut head = Vec::new();
    let mut chunk = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        if head.len() > 16 * 1024 {
            return None;
        }
        let left = deadline.checked_duration_since(Instant::now())?;
        if left.is_zero() {
            return None;
        }
        stream.set_read_timeout(Some(left)).ok()?;
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        head.extend_from_slice(&chunk[..n]);
    }
    let end = head.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let rest = head.split_off(end);
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target, version) = (first.next()?, first.next()?, first.next()?);
    if !version.starts_with("HTTP/1.") || first.next().is_some() {
        return None;
    }
    let headers = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .collect();
    Some((
        Request {
            method: method.to_string(),
            target: target.to_string(),
            headers,
        },
        rest,
    ))
}

/// Writes a response and closes the connection.
pub(crate) fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    stream.shutdown(Shutdown::Write)
}

/// Opens `url` in the default browser: `open` on macOS, `cmd /c start "" URL` on Windows,
/// `xdg-open` elsewhere.
pub fn launch_browser(url: &str) -> io::Result<()> {
    use std::process::{Command, Stdio};
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/c", "start", "", url]);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::{API_VERSION, Root, bundle, header_value, resolve_assets};

    /// Built-in files from `(path, contents)`, gzipped as build.rs stores them.
    fn built(files: &[(&'static str, &[u8])]) -> bundle::Files {
        let mut list: Vec<(&'static str, &'static [u8])> = files
            .iter()
            .map(|(path, bytes)| {
                let mut gz = flate2::write::GzEncoder::new(Vec::new(), Default::default());
                gz.write_all(bytes).unwrap();
                let gz: &'static [u8] = Vec::leak(gz.finish().unwrap());
                (*path, gz)
            })
            .collect();
        list.sort();
        Vec::leak(list)
    }

    fn app(api: u32) -> bundle::Files {
        let manifest = format!(r#"{{"name":"specify-ui-app","version":"1.0.0","api":{api}}}"#);
        built(&[
            ("index.html", b"app"),
            ("specify-ui-app.json", manifest.as_bytes()),
        ])
    }

    fn refused(result: Result<super::Assets, super::Error>) -> String {
        match result {
            Err(super::Error::Refused(message)) => message,
            other => panic!("not refused: {other:?}"),
        }
    }

    #[test]
    fn a_build_without_the_app_asks_for_one() {
        let message = refused(resolve_assets(None, None, None, None));
        assert!(
            message.starts_with("this specify-core was built without the Specify app"),
            "{message}"
        );
        assert!(message.contains("pip install specify-core"), "{message}");
        assert!(
            message.ends_with(": pass --app DIR / set SPECIFY_APP_DIR"),
            "{message}"
        );
        // An engine alone is not enough.
        let engine = built(&[("specify_core_wasm.js", b"glue")]);
        assert!(
            refused(resolve_assets(None, None, None, Some(engine))).ends_with("SPECIFY_APP_DIR")
        );
    }

    #[test]
    fn a_built_in_app_needs_its_engine() {
        let message = refused(resolve_assets(None, None, Some(app(API_VERSION)), None));
        assert!(
            message.starts_with("this specify-core was built without the Specify WASM engine"),
            "{message}"
        );
        assert!(
            message.ends_with(": pass --wasm DIR / set SPECIFY_WASM_DIR"),
            "{message}"
        );
        let engine = built(&[("specify_core_wasm.js", b"glue")]);
        let assets = resolve_assets(None, None, Some(app(API_VERSION)), Some(engine)).unwrap();
        assert!(matches!(assets.app, Root::BuiltIn(_)));
        assert!(matches!(assets.wasm, Some(Root::BuiltIn(_))));
        let (bytes, content_type) = assets.wasm.unwrap().file("specify_core_wasm.js").unwrap();
        assert_eq!(bytes, b"glue");
        assert_eq!(content_type, "text/javascript; charset=utf-8");
    }

    #[test]
    fn the_built_in_app_must_speak_this_session_api() {
        let engine = built(&[("specify_core_wasm.js", b"glue")]);
        let message = refused(resolve_assets(
            None,
            None,
            Some(app(API_VERSION + 1)),
            Some(engine),
        ));
        assert!(message.contains("this app speaks session API"), "{message}");
        let without = built(&[("index.html", b"app")]);
        let message = refused(resolve_assets(None, None, Some(without), Some(engine)));
        assert!(
            message.contains("the built-in app has no manifest"),
            "{message}"
        );
    }

    #[test]
    fn header_values_keep_plain_paths_and_encode_the_rest() {
        assert_eq!(
            header_value("/a/b c/part.specified-2.step"),
            "/a/b c/part.specified-2.step"
        );
        assert_eq!(
            header_value("/a/100%/p\u{e9}\r\n.step"),
            "/a/100%25/p%C3%A9%0D%0A.step"
        );
    }
}
