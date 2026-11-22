//! `specify-core open` (src/open.rs, src/session.rs, docs/open.md): the local Specify app,
//! against the built binary on a real loopback port with a stub app and engine, and
//! `--no-browser` so no browser opens. The app, engine, file and config are served under the
//! token at exactly `Host: 127.0.0.1:<port>`; the session's answers are read and streamed;
//! saves come only from the app's own origin, apply the page's edit list to the file all or
//! nothing (as `specify-core write STEP EDITS.json`), and never overwrite (unless
//! `--in-place`, which keeps a `.bak`); `closed` stops the server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use specify_core::open::{self, App, Assets, Options};
use specify_core::session::{Opened as OpenedFile, Session};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/thread-depth-alpha7.step.gz")
}

fn revision(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/revisions")
        .join(name)
}

/// The fixture's STEP text.
fn step_text() -> Vec<u8> {
    gunzip(&std::fs::read(fixture()).unwrap())
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut text = Vec::new();
    flate2::read::MultiGzDecoder::new(bytes)
        .read_to_end(&mut text)
        .unwrap();
    text
}

/// A scratch directory holding a stub app (`app/`), a stub engine (`pkg/`), a file outside
/// both (`secret.txt`) and a plain STEP input (`work/part.step`); removed when dropped.
struct Scratch {
    root: PathBuf,
    /// The same directory as first named, not resolved (on macOS `/var/…`, not
    /// `/private/var/…`).
    given: PathBuf,
}

impl Scratch {
    fn new(test: &str) -> Scratch {
        let root =
            std::env::temp_dir().join(format!("specify-core-open-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["app/assets", "pkg", "work"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let given = root.clone();
        let root = std::fs::canonicalize(root).unwrap();
        let write = |p: &str, b: &[u8]| std::fs::write(root.join(p), b).unwrap();
        write("app/index.html", b"<!doctype html><title>stub app</title>");
        write("app/app.css", b"body{}");
        write("app/assets/main.mjs", b"export {};");
        for ext in ["ico", "jpg", "jpeg", "webp", "woff", "ttf", "map", "woff2"] {
            write(&format!("app/assets/a.{ext}"), ext.as_bytes());
        }
        write("pkg/specify_core_wasm.js", b"export default 1;");
        write("pkg/specify_core_wasm_bg.wasm", b"\0asm\x01\0\0\0");
        write("secret.txt", b"secret");
        std::fs::write(root.join("work/part.step"), step_text()).unwrap();
        Scratch { root, given }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn input(&self) -> PathBuf {
        self.path("work/part.step")
    }

    /// The names in `work/`, sorted.
    fn work(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.path("work"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn assets(&self) -> Assets {
        open::assets(Some(&self.path("app")), Some(&self.path("pkg"))).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// `specify-core open <inputs> --no-browser` with the scratch app and engine.
fn command(inputs: &[&Path], scratch: &Scratch, extra: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_specify-core"));
    c.arg("open");
    if let [old, new] = inputs {
        c.arg("--diff").arg(old).arg(new);
    } else {
        c.args(inputs);
    }
    c.arg("--no-browser")
        .arg("--app")
        .arg(scratch.path("app"))
        .arg("--wasm")
        .arg(scratch.path("pkg"))
        .args(extra)
        .env_remove("SPECIFY_APP_DIR")
        .env_remove("SPECIFY_WASM_DIR");
    c
}

/// A running `specify-core open ... --no-browser`, killed if a test fails before it exits.
struct Opened {
    child: Child,
    stdout: BufReader<ChildStdout>,
    port: u16,
    /// `/<token>/`.
    prefix: String,
}

impl Opened {
    fn start(step: &Path, scratch: &Scratch, extra: &[&str]) -> Opened {
        Opened::run(&[step], scratch, extra)
    }

    /// `open --diff OLD NEW`.
    fn diff(old: &Path, new: &Path, scratch: &Scratch, extra: &[&str]) -> Opened {
        Opened::run(&[old, new], scratch, extra)
    }

    fn run(inputs: &[&Path], scratch: &Scratch, extra: &[&str]) -> Opened {
        Self::from_command(inputs.last().unwrap(), command(inputs, scratch, extra))
    }

    fn from_command(step: &Path, mut command: Command) -> Opened {
        let name = step.file_name().unwrap().to_string_lossy().into_owned();
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let url = line
            .trim()
            .strip_prefix(&format!("Open {name}: "))
            .unwrap_or_else(|| panic!("the address line: {line:?}"));
        let rest = url.strip_prefix("http://127.0.0.1:").expect("on 127.0.0.1");
        let (port, prefix) = rest.split_once('/').unwrap();
        let prefix = format!("/{prefix}");
        let token = prefix.trim_matches('/');
        assert_eq!(token.len(), 32, "{url}");
        assert!(
            token.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "{token}"
        );
        assert!(prefix.ends_with('/'), "{url}");
        Opened {
            child,
            stdout,
            port: port.parse().unwrap(),
            prefix,
        }
    }

    fn page(&self) -> Page {
        Page {
            port: self.port,
            prefix: self.prefix.clone(),
        }
    }

    fn get(&self, rel: &str) -> Response {
        self.page().get(rel)
    }

    fn config(&self) -> Value {
        self.page().config()
    }

    /// A save of the edit list `edits` from the app's own page.
    fn save(&self, rev: u64, edits: Value) -> Response {
        self.page().save(rev, edits)
    }

    /// The next line on stdout.
    fn line(&mut self) -> String {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        line
    }

    fn still_running(&mut self) -> bool {
        std::thread::sleep(Duration::from_millis(100));
        self.child.try_wait().unwrap().is_none()
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A page server's address, and the requests the app makes.
#[derive(Clone)]
struct Page {
    port: u16,
    /// `/<token>/`.
    prefix: String,
}

impl Page {
    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn path(&self, rel: &str) -> String {
        format!("{}{rel}", self.prefix)
    }

    fn get(&self, rel: &str) -> Response {
        request(
            self.port,
            "GET",
            &self.path(rel),
            &[("Host", &self.host())],
            b"",
        )
    }

    fn config(&self) -> Value {
        let got = self.get("config");
        assert_eq!(got.status, 200);
        assert_eq!(got.header("content-type"), Some("application/json"));
        got.json()
    }

    fn answers(&self) -> Value {
        let got = self.get("answers");
        assert_eq!(got.status, 200);
        got.json()
    }

    /// A JSON request from the app's own page.
    fn send(&self, method: &str, rel: &str, body: &Value) -> Response {
        let (host, origin) = (self.host(), self.origin());
        request(
            self.port,
            method,
            &self.path(rel),
            &[
                ("Host", &host),
                ("Origin", &origin),
                ("Content-Type", "application/json"),
            ],
            body.to_string().as_bytes(),
        )
    }

    /// A save from the page of the edit list `edits`.
    fn save(&self, rev: u64, edits: Value) -> Response {
        self.send("POST", "save", &json!({"rev": rev, "edits": edits}))
    }

    fn rev(&self) -> u64 {
        self.answers()["rev"].as_u64().unwrap()
    }

    fn events(&self) -> Events {
        Events::open(self)
    }
}

/// An open `events` stream, read event by event.
struct Events {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl Events {
    fn open(page: &Page) -> Events {
        let mut stream = TcpStream::connect(("127.0.0.1", page.port)).unwrap();
        write!(
            stream,
            "GET {} HTTP/1.1\r\nHost: {}\r\n\r\n",
            page.path("events"),
            page.host()
        )
        .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut events = Events {
            stream,
            buffer: Vec::new(),
        };
        let head = events.block();
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(
            head.to_ascii_lowercase()
                .contains("content-type: text/event-stream"),
            "{head}"
        );
        events
    }

    /// The next block up to a blank line.
    fn block(&mut self) -> String {
        loop {
            let text = String::from_utf8_lossy(&self.buffer).into_owned();
            let ends = ["\r\n\r\n", "\n\n"]
                .iter()
                .filter_map(|end| text.find(end).map(|i| (i, end.len())))
                .min();
            if let Some((i, len)) = ends {
                self.buffer.drain(..i + len);
                return text[..i].to_string();
            }
            let mut chunk = [0u8; 4096];
            let n = self.stream.read(&mut chunk).expect("an event in time");
            assert!(n > 0, "the stream ended");
            self.buffer.extend_from_slice(&chunk[..n]);
        }
    }

    /// The next event (comments skipped): its name and data.
    fn next(&mut self) -> (String, Value) {
        loop {
            let block = self.block();
            let mut name = String::new();
            let mut data = String::new();
            for line in block.lines() {
                if let Some(n) = line.strip_prefix("event: ") {
                    name = n.to_string();
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data.push_str(d);
                }
            }
            if !name.is_empty() {
                return (name, serde_json::from_str(&data).unwrap());
            }
        }
    }
}

/// A response: status code, headers (names lowercased) and body.
struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&self.body)))
    }
}

/// One raw HTTP/1.1 request to the port, with exactly these headers (and a Content-Length when
/// there is a body or the method is POST or PUT and none is given).
fn request(
    port: u16,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Response {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the port is open");
    let mut head = format!("{method} {target} HTTP/1.1\r\n");
    for (n, v) in headers {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    let has_length = headers
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case("content-length"));
    if !has_length && (method == "POST" || method == "PUT" || !body.is_empty()) {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    // A refusal may answer before the body is read; the response is what is checked.
    let _ = stream.write_all(body);
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a head");
    let text = String::from_utf8(raw[..split].to_vec()).unwrap();
    let mut lines = text.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Response {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

/// A refused request: what it is, its target, its headers and the status it gets.
type Refused<'a> = (&'a str, String, Vec<(&'a str, &'a str)>, u16);

/// A refused save: what it is, its headers, its body and the status it gets.
type RefusedSave<'a> = (&'a str, Vec<(&'a str, &'a str)>, Vec<u8>, u16);

/// A session of the scratch input, as `specify-core open` makes it.
fn session_of(path: &Path) -> Arc<Mutex<Session>> {
    let session = Session::open(OpenedFile::read(path).unwrap(), false).unwrap();
    Arc::new(Mutex::new(session))
}

/// An [`App`] served on a thread with these delays, its notices collected.
struct Served {
    port: u16,
    /// `/<token>/`.
    prefix: String,
    notices: Arc<Mutex<Vec<String>>>,
    thread: std::thread::JoinHandle<()>,
}

impl Served {
    fn start(scratch: &Scratch, close_delay: Duration, idle: Duration) -> Served {
        let app = App::new(
            session_of(&scratch.input()),
            scratch.assets(),
            &Options::default(),
        )
        .unwrap();
        let port = app.port();
        let prefix = app
            .url()
            .strip_prefix(&format!("http://127.0.0.1:{port}"))
            .unwrap()
            .to_string();
        let notices = Arc::new(Mutex::new(Vec::new()));
        let told = notices.clone();
        let thread = std::thread::spawn(move || {
            app.serve(close_delay, idle, |n| {
                told.lock().unwrap().push(n.to_string())
            })
            .unwrap()
        });
        Served {
            port,
            prefix,
            notices,
            thread,
        }
    }

    fn page(&self) -> Page {
        Page {
            port: self.port,
            prefix: self.prefix.clone(),
        }
    }

    fn post(&self, rel: &str, origin: Option<&str>) -> Response {
        let host = self.page().host();
        let mut headers = vec![("Host", host.as_str())];
        if let Some(o) = origin {
            headers.push(("Origin", o));
        }
        request(
            self.port,
            "POST",
            &format!("{}{rel}", self.prefix),
            &headers,
            b"",
        )
    }

    /// `closed` from the app's own page.
    fn closed(&self) -> Response {
        let origin = format!("http://127.0.0.1:{}", self.port);
        self.post("closed", Some(&origin))
    }

    /// An open page's heartbeat.
    fn heartbeat(&self) {
        assert_eq!(self.page().get("config").status, 200);
    }

    /// Heartbeats every `every` for `during`; the server must still be serving after it.
    fn heartbeats(&self, every: Duration, during: Duration) {
        let end = Instant::now() + during;
        while Instant::now() < end {
            self.heartbeat();
            std::thread::sleep(every);
        }
        assert!(!self.thread.is_finished(), "the heartbeats kept it running");
    }

    /// Waits up to `within` for the server to stop: its notices.
    fn stopped(self, within: Duration) -> Vec<String> {
        let deadline = Instant::now() + within;
        while !self.thread.is_finished() {
            assert!(Instant::now() < deadline, "the server did not stop");
            std::thread::sleep(Duration::from_millis(20));
        }
        self.thread.join().unwrap();
        assert!(
            TcpStream::connect(("127.0.0.1", self.port)).is_err(),
            "the port is closed"
        );
        self.notices.lock().unwrap().clone()
    }
}

/// `analyse`'s JSON of `path` (part 0).
fn analysis_of(path: &Path) -> Value {
    specify_core::api::analyse_file(path, None).unwrap()
}

/// How many times `needle` occurs in the STEP text of `path` (gzipped or not).
fn occurrences(path: &Path, needle: &str) -> usize {
    let bytes = std::fs::read(path).unwrap();
    let text = if bytes.starts_with(&[0x1f, 0x8b]) {
        gunzip(&bytes)
    } else {
        bytes
    };
    String::from_utf8_lossy(&text).matches(needle).count()
}

fn is_step(path: &Path) -> bool {
    std::fs::read(path).unwrap().starts_with(b"ISO-10303-21;")
}

/// An edit list that stores a decision in the fixture's file (decided none leaves no PMI, so
/// it is stored with Specify's answers, and the session's answers change: a new rev).
fn decide_coating() -> Value {
    json!({"none": ["part.coating"]})
}

/// A write report without where it was written.
fn report_without_output(report: &Value) -> Value {
    let mut r = report.as_object().unwrap().clone();
    r.remove("output");
    Value::Object(r)
}

#[test]
fn the_app_engine_config_and_file_are_served_under_the_token() {
    let scratch = Scratch::new("served");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);

    let index = opened.get("");
    assert_eq!(index.status, 200);
    assert_eq!(index.body, b"<!doctype html><title>stub app</title>");
    assert_eq!(
        index.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(index.header("cache-control"), Some("no-store"));
    assert_eq!(index.header("x-content-type-options"), Some("nosniff"));
    let csp = index.header("content-security-policy").unwrap();
    assert!(csp.contains("default-src 'self'"), "{csp}");
    assert!(csp.contains("'wasm-unsafe-eval'"), "{csp}");
    assert!(csp.contains("font-src 'self' data:"), "{csp}");
    assert_eq!(opened.get("index.html").body, index.body);

    for (rel, content_type, body) in [
        ("app.css", "text/css; charset=utf-8", &b"body{}"[..]),
        (
            "assets/main.mjs",
            "text/javascript; charset=utf-8",
            b"export {};",
        ),
        (
            "wasm/specify_core_wasm.js",
            "text/javascript; charset=utf-8",
            b"export default 1;",
        ),
        (
            "wasm/specify_core_wasm_bg.wasm",
            "application/wasm",
            b"\0asm\x01\0\0\0",
        ),
        ("assets/a.ico", "image/x-icon", b"ico"),
        ("assets/a.jpg", "image/jpeg", b"jpg"),
        ("assets/a.jpeg", "image/jpeg", b"jpeg"),
        ("assets/a.webp", "image/webp", b"webp"),
        ("assets/a.woff", "font/woff", b"woff"),
        ("assets/a.woff2", "font/woff2", b"woff2"),
        ("assets/a.ttf", "font/ttf", b"ttf"),
        ("assets/a.map", "application/json", b"map"),
    ] {
        let got = opened.get(rel);
        assert_eq!(got.status, 200, "{rel}");
        assert_eq!(got.header("content-type"), Some(content_type), "{rel}");
        assert_eq!(got.body, body, "{rel}");
    }

    // The token without its slash is sent to the app's address.
    let page = opened.page();
    let bare = request(
        opened.port,
        "GET",
        opened.prefix.trim_end_matches('/'),
        &[("Host", &page.host())],
        b"",
    );
    assert_eq!(bare.status, 308);
    assert_eq!(bare.header("location"), Some(opened.prefix.as_str()));

    let file = opened.get("file");
    assert_eq!(file.status, 200);
    assert_eq!(file.body, std::fs::read(scratch.input()).unwrap());
    assert_eq!(
        file.header("content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(opened.get("file?rev=new").body, file.body);
    // Not a diff: no OLD and no report.
    assert_eq!(opened.get("file?rev=old").status, 404);
    assert_eq!(opened.get("diff").status, 404);

    let config = opened.config();
    // Contract api 2 (0.3.0): Save sends an edit list, and there is no `PUT answers`.
    assert_eq!(config["api"], 2);
    assert_eq!(config["mode"], "specify");
    assert_eq!(config["name"], "part.step");
    assert_eq!(config["path"], scratch.input().display().to_string());
    assert_eq!(config["part"], Value::Null);
    assert_eq!(config["oldName"], Value::Null);
    assert_eq!(config["oldPath"], Value::Null);
    assert_eq!(config["readOnly"], false);
    assert_eq!(config["wasmUrl"], "wasm/specify_core_wasm.js");
    assert_eq!(config["specifyCore"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        config["saveTarget"],
        scratch
            .path("work/part.specified.step")
            .display()
            .to_string()
    );
    let keys: Vec<&String> = config.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "api",
            "mode",
            "name",
            "path",
            "part",
            "oldName",
            "oldPath",
            "readOnly",
            "wasmUrl",
            "saveTarget",
            "specifyCore"
        ]
    );
    // The fixture is a file Specify wrote: the session starts from the answers it stores, as
    // `analyse` resumes them (ADR P11: none here, as the file states its material and thread).
    let resumed = analysis_of(&scratch.input())["resume"]["answers"].clone();
    assert_eq!(
        opened.page().answers(),
        json!({"rev": 0, "parts": {"0": resumed}})
    );
}

#[test]
fn other_hosts_tokens_origins_and_paths_outside_the_app_are_refused() {
    let scratch = Scratch::new("refused");
    #[cfg(unix)]
    std::os::unix::fs::symlink(scratch.path("secret.txt"), scratch.path("app/link.txt")).unwrap();
    let mut opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let host = page.host();
    let other_port = format!("127.0.0.1:{}", opened.port.wrapping_add(1));
    let localhost = format!("localhost:{}", opened.port);
    let index = page.path("");
    let other_token = format!("/{}/", "0".repeat(32));
    let other_token_file = format!("/{}/file", "0".repeat(32));
    let cases: Vec<Refused> = vec![
        (
            "host localhost",
            index.clone(),
            vec![("Host", &localhost)],
            403,
        ),
        (
            "host on another port",
            index.clone(),
            vec![("Host", &other_port)],
            403,
        ),
        ("no host", index.clone(), vec![], 403),
        (
            "rebinding host",
            index.clone(),
            vec![("Host", "attacker.example:80")],
            403,
        ),
        (
            "another origin's page",
            page.path("file"),
            vec![("Host", &host), ("Origin", "https://example.com")],
            403,
        ),
        (
            "another origin reading the answers",
            page.path("answers"),
            vec![("Host", &host), ("Origin", "https://example.com")],
            403,
        ),
        (
            "another origin's event stream",
            page.path("events"),
            vec![("Host", &host), ("Origin", "https://example.com")],
            403,
        ),
        ("wrong token", other_token, vec![("Host", &host)], 404),
        (
            "wrong token's file",
            other_token_file,
            vec![("Host", &host)],
            404,
        ),
        ("the root", "/".to_string(), vec![("Host", &host)], 404),
        (
            "dot-dot",
            page.path("../secret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "encoded dot-dot",
            page.path("%2e%2e/secret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "encoded slash",
            page.path("assets%2f..%2f..%2fsecret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "out of the engine",
            page.path("wasm/../../secret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "backslash",
            page.path("assets%5c..%5c..%5csecret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "a directory",
            page.path("assets/"),
            vec![("Host", &host)],
            404,
        ),
        (
            "a directory without its slash",
            page.path("assets"),
            vec![("Host", &host)],
            404,
        ),
        (
            "the engine directory without its slash",
            page.path("wasm"),
            vec![("Host", &host)],
            404,
        ),
        (
            "the token and more",
            format!("{}x/", opened.prefix.trim_end_matches('/')),
            vec![("Host", &host)],
            404,
        ),
        (
            "the token cut short",
            format!("{}/", &opened.prefix[..opened.prefix.len() - 2]),
            vec![("Host", &host)],
            404,
        ),
        (
            "the token cut short, no slash",
            opened.prefix[..opened.prefix.len() - 2].to_string(),
            vec![("Host", &host)],
            404,
        ),
        (
            "a missing file",
            page.path("missing.js"),
            vec![("Host", &host)],
            404,
        ),
        #[cfg(unix)]
        (
            "a link leading out",
            page.path("link.txt"),
            vec![("Host", &host)],
            404,
        ),
    ];
    for (what, target, headers, status) in cases {
        let got = request(opened.port, "GET", &target, &headers, b"");
        assert_eq!(got.status, status, "{what}");
        assert!(got.body.is_empty(), "{what}: nothing served");
    }
    let put = request(
        opened.port,
        "PUT",
        &page.path("file"),
        &[("Host", &host)],
        b"",
    );
    assert_eq!(put.status, 405);
    let post = request(
        opened.port,
        "POST",
        &page.path("answers"),
        &[("Host", &host)],
        b"",
    );
    assert_eq!(post.status, 405);
    assert_eq!(post.header("allow"), Some("GET"));
    // Contract api 2: the page no longer changes the session's answers.
    let put = page.send(
        "PUT",
        "answers",
        &json!({"rev": 0, "part": 0, "answers": {"part.material": "6061-T6"}}),
    );
    assert_eq!(put.status, 405);
    assert_eq!(put.header("allow"), Some("GET"));
    assert_eq!(page.rev(), 0);
    assert!(opened.still_running());
}

#[test]
fn saves_update_the_sessions_copy_and_never_overwrite_another_file() {
    let scratch = Scratch::new("save");
    let input = std::fs::read(scratch.input()).unwrap();
    let mut opened = Opened::start(&scratch.input(), &scratch, &[]);
    let work = |n: &str| scratch.path("work").join(n);

    let first = opened.save(0, json!({}));
    assert_eq!(
        first.status,
        200,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    let first = first.json();
    assert_eq!(
        first["path"],
        work("part.specified.step").display().to_string()
    );
    assert!(is_step(&work("part.specified.step")));
    assert_eq!(
        first["bytes"],
        std::fs::metadata(work("part.specified.step"))
            .unwrap()
            .len()
    );
    assert_eq!(first["report"]["output"], first["path"]);
    assert_eq!(
        opened.line(),
        format!(
            "Saved {} ({} bytes).\n",
            first["path"].as_str().unwrap(),
            first["bytes"]
        )
    );
    // Later saves update the copy this session wrote (#217).
    assert_eq!(
        opened.config()["saveTarget"],
        work("part.specified.step").display().to_string()
    );
    let second = opened.save(0, json!({}));
    assert_eq!(second.status, 200);
    assert_eq!(
        second.json()["path"],
        work("part.specified.step").display().to_string()
    );
    assert_eq!(scratch.work(), ["part.specified.step", "part.step"]);

    // A copy changed by someone else is never written over, nor a file already at the next
    // name: the save takes the first free one.
    std::fs::write(work("part.specified.step"), b"edited").unwrap();
    std::fs::write(work("part.specified-2.step"), b"mine").unwrap();
    assert_eq!(
        opened.config()["saveTarget"],
        work("part.specified-3.step").display().to_string()
    );
    let third = opened.save(0, json!({}));
    assert_eq!(third.status, 200);
    assert_eq!(
        third.json()["path"],
        work("part.specified-3.step").display().to_string()
    );
    assert_eq!(
        std::fs::read(work("part.specified.step")).unwrap(),
        b"edited"
    );
    assert_eq!(
        std::fs::read(work("part.specified-2.step")).unwrap(),
        b"mine"
    );
    // And the session's new copy is the one updated from then on.
    let fourth = opened.save(0, json!({}));
    assert_eq!(fourth.status, 200);
    assert_eq!(
        fourth.json()["path"],
        work("part.specified-3.step").display().to_string()
    );
    assert_eq!(std::fs::read(scratch.input()).unwrap(), input);
    assert_eq!(
        scratch.work(),
        [
            "part.specified-2.step",
            "part.specified-3.step",
            "part.specified.step",
            "part.step"
        ],
        "no temporary file left"
    );
    assert!(opened.still_running());
}

#[test]
fn a_save_from_elsewhere_of_another_type_without_edits_or_too_large_is_refused() {
    let scratch = Scratch::new("save-refused");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let (host, origin) = (page.host(), page.origin());
    let localhost_origin = format!("http://localhost:{}", opened.port);
    let target = page.path("save");
    let json_type = ("Content-Type", "application/json");
    let body = br#"{"rev": 0, "edits": {}}"#.to_vec();
    let over = (16 * 1024 * 1024 + 1).to_string();
    let cases: Vec<RefusedSave> = vec![
        (
            "no origin",
            vec![("Host", &host), json_type],
            body.clone(),
            403,
        ),
        (
            "another origin",
            vec![
                ("Host", &host),
                ("Origin", "https://example.com"),
                json_type,
            ],
            body.clone(),
            403,
        ),
        (
            "localhost origin",
            vec![("Host", &host), ("Origin", &localhost_origin), json_type],
            body.clone(),
            403,
        ),
        (
            "null origin",
            vec![("Host", &host), ("Origin", "null"), json_type],
            body.clone(),
            403,
        ),
        (
            "STEP bytes, as before",
            vec![
                ("Host", &host),
                ("Origin", &origin),
                ("Content-Type", "application/octet-stream"),
            ],
            step_text(),
            415,
        ),
        (
            "no content type",
            vec![("Host", &host), ("Origin", &origin)],
            body.clone(),
            415,
        ),
        (
            "chunked",
            vec![
                ("Host", &host),
                ("Origin", &origin),
                json_type,
                ("Transfer-Encoding", "chunked"),
            ],
            body.clone(),
            411,
        ),
        (
            "not JSON",
            vec![("Host", &host), ("Origin", &origin), json_type],
            b"<html>not a save</html>".to_vec(),
            400,
        ),
        (
            "edits absent",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"rev": 0}"#.to_vec(),
            400,
        ),
        (
            "acceptDefaults, as api 1 sent",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"rev": 0, "acceptDefaults": {}}"#.to_vec(),
            400,
        ),
        (
            "edits a list",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"rev": 0, "edits": []}"#.to_vec(),
            400,
        ),
        (
            "edits a bool",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"rev": 0, "edits": true}"#.to_vec(),
            400,
        ),
        (
            "rev absent",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"edits": {}}"#.to_vec(),
            400,
        ),
        (
            "over 16 MB",
            vec![
                ("Host", &host),
                ("Origin", &origin),
                json_type,
                ("Content-Length", &over),
            ],
            Vec::new(),
            413,
        ),
    ];
    for (what, headers, body, status) in cases {
        let got = request(opened.port, "POST", &target, &headers, &body);
        assert_eq!(got.status, status, "{what}");
        assert!(got.json()["error"].is_string(), "{what}");
    }
    // Not an edit list: every part that does not parse is named.
    let malformed = page.save(0, json!({"fill": [], "remove": ["#1"]}));
    assert_eq!(malformed.status, 400);
    let failures = malformed.json()["failures"].clone();
    let entries: Vec<&str> = failures
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["entry"].as_str().unwrap())
        .collect();
    assert_eq!(entries, ["fill", "remove"], "{failures}");
    // A list one of whose entries cannot be applied: all or nothing, every failing entry named,
    // nothing written and the session unchanged.
    let refused = page.save(
        0,
        json!({"fill": {"part.material": "6061-T6"}, "none": ["part.coating"],
               "items": {"remove": ["#404"]}}),
    );
    assert_eq!(refused.status, 422);
    let refused = refused.json();
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("nothing was written")
    );
    let entries: Vec<&str> = refused["failures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["entry"].as_str().unwrap())
        .collect();
    assert_eq!(
        entries,
        [r#"fill["part.material"]"#, r##"items.remove["#404"]"##],
        "{refused}"
    );
    assert_eq!(page.rev(), 0);
    let get = request(opened.port, "GET", &target, &[("Host", &host)], b"");
    assert_eq!(get.status, 405);
    assert_eq!(scratch.work(), ["part.step"], "nothing written");
    // A rev the session has moved on from: refused, and nothing more written.
    assert_eq!(page.save(0, decide_coating()).status, 200);
    assert_eq!(page.rev(), 1);
    let stale = page.save(0, json!({"none": ["part.heat_treatment"]}));
    assert_eq!(stale.status, 409);
    assert_eq!(stale.json()["rev"], 1);
    assert!(stale.json()["error"].is_string());
    let written = scratch.path("work/part.specified.step");
    let table = analysis_of(&written)["table"].clone();
    assert!(
        !table["decided_none"]
            .as_array()
            .unwrap()
            .contains(&json!("part.heat_treatment")),
        "{table}"
    );
}

#[test]
fn read_only_refuses_saves_and_writes_nothing() {
    let scratch = Scratch::new("read-only");
    let opened = Opened::start(&scratch.input(), &scratch, &["--read-only"]);
    let config = opened.config();
    assert_eq!(config["mode"], "read-only");
    assert_eq!(config["readOnly"], true);
    assert_eq!(config["saveTarget"], Value::Null);
    let got = opened.save(0, json!({}));
    assert_eq!(got.status, 409);
    assert!(got.json()["error"].is_string());
    let decided = opened.save(0, decide_coating());
    assert_eq!(decided.status, 409);
    assert_eq!(opened.page().answers()["rev"], 0);
    assert_eq!(scratch.work(), ["part.step"]);
}

/// `file?rev=saved`: 404 before a save, then exactly the bytes the last save wrote (read from
/// disk, its path in `X-Specify-Path`), following each save; a copy changed on disk is 409
/// and one removed 404, never other bytes.
#[test]
fn rev_saved_serves_what_the_last_save_wrote() {
    let scratch = Scratch::new("rev-saved");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let work = |n: &str| scratch.path("work").join(n);
    let before = opened.get("file?rev=saved");
    assert_eq!(before.status, 404);
    assert!(before.json()["error"].is_string());

    let saved = |expected: &Path| {
        let got = opened.get("file?rev=saved");
        assert_eq!(got.status, 200, "{}", String::from_utf8_lossy(&got.body));
        assert_eq!(got.header("content-type"), Some("application/octet-stream"));
        assert_eq!(
            got.header("x-specify-path"),
            Some(expected.display().to_string().as_str())
        );
        assert_eq!(got.body, std::fs::read(expected).unwrap());
        got.body
    };

    let first = opened.save(0, json!({}));
    assert_eq!(first.status, 200);
    assert_eq!(
        first.json()["path"],
        work("part.specified.step").display().to_string()
    );
    let first_bytes = saved(&work("part.specified.step"));
    assert!(first_bytes.starts_with(b"ISO-10303-21;"));
    // The same rules as every GET: another page's Origin, or another Host, is refused.
    let page = opened.page();
    let (host, path) = (page.host(), page.path("file?rev=saved"));
    let foreign = request(
        page.port,
        "GET",
        &path,
        &[("Host", &host), ("Origin", "http://evil.example")],
        b"",
    );
    assert_eq!(foreign.status, 403);
    assert!(foreign.body.is_empty());
    let rebound = request(page.port, "GET", &path, &[("Host", "evil.example")], b"");
    assert_eq!(rebound.status, 403);
    // `file` is still the input as opened.
    assert_eq!(
        opened.get("file").body,
        std::fs::read(scratch.input()).unwrap()
    );

    // A later save updates the session's copy: `rev=saved` follows it.
    let second = opened.save(0, decide_coating());
    assert_eq!(second.status, 200);
    assert_eq!(
        second.json()["path"],
        work("part.specified.step").display().to_string()
    );
    let second_bytes = saved(&work("part.specified.step"));
    assert_ne!(
        second_bytes, first_bytes,
        "the second save wrote the decision"
    );

    // Changed on disk since the save: refused, never the other bytes.
    let mut changed = second_bytes.clone();
    changed.extend_from_slice(b"/* edited */\n");
    std::fs::write(work("part.specified.step"), &changed).unwrap();
    let got = opened.get("file?rev=saved");
    assert_eq!(got.status, 409);
    assert_eq!(got.header("content-type"), Some("application/json"));
    let error = got.json();
    assert!(
        error["error"].as_str().unwrap().contains("changed"),
        "{error}"
    );
    assert_eq!(
        error["path"],
        work("part.specified.step").display().to_string()
    );
    // Put back as saved, it is served again; removed, it is 404.
    std::fs::write(work("part.specified.step"), &second_bytes).unwrap();
    saved(&work("part.specified.step"));
    std::fs::remove_file(work("part.specified.step")).unwrap();
    let got = opened.get("file?rev=saved");
    assert_eq!(got.status, 404);
    assert!(
        got.json()["error"]
            .as_str()
            .unwrap()
            .contains("no longer exists")
    );
}

/// `file?rev=saved` after an in-place save is the input as that save left it.
#[test]
fn rev_saved_after_an_in_place_save_is_the_input() {
    let scratch = Scratch::new("rev-saved-in-place");
    let input = std::fs::read(scratch.input()).unwrap();
    let opened = Opened::start(&scratch.input(), &scratch, &["--in-place"]);
    assert_eq!(opened.get("file?rev=saved").status, 404);
    let got = opened.save(0, decide_coating());
    assert_eq!(got.status, 200);
    assert_eq!(got.json()["path"], scratch.input().display().to_string());
    let now = std::fs::read(scratch.input()).unwrap();
    assert_ne!(now, input);
    let got = opened.get("file?rev=saved");
    assert_eq!(got.status, 200);
    assert_eq!(
        got.header("x-specify-path"),
        Some(scratch.input().display().to_string().as_str())
    );
    assert_eq!(got.body, now);
    assert_eq!(opened.get("file").body, now);
}

/// `--read-only` never saves: `file?rev=saved` is always 404.
#[test]
fn rev_saved_read_only_is_not_found() {
    let scratch = Scratch::new("rev-saved-read-only");
    let opened = Opened::start(&scratch.input(), &scratch, &["--read-only"]);
    assert_eq!(opened.get("file?rev=saved").status, 404);
    assert_eq!(opened.save(0, json!({})).status, 409);
    assert_eq!(opened.get("file?rev=saved").status, 404);
}

/// A save made outside the page (MCP's `save`, on the same session) is what `rev=saved` serves.
#[test]
fn rev_saved_follows_a_save_from_the_agent() {
    let scratch = Scratch::new("rev-saved-agent");
    let session = session_of(&scratch.input());
    let app = App::new(Arc::clone(&session), scratch.assets(), &Options::default()).unwrap();
    let port = app.port();
    let prefix = app
        .url()
        .strip_prefix(&format!("http://127.0.0.1:{port}"))
        .unwrap()
        .to_string();
    std::thread::spawn(move || {
        let _ = app.serve(Duration::from_secs(60), Duration::from_secs(60), |_| {});
    });
    let page = Page { port, prefix };
    assert_eq!(page.get("file?rev=saved").status, 404);
    let saved =
        specify_core::session::save_edits(&session, None, &Default::default(), "mcp").unwrap();
    let written = scratch.path("work/part.specified.step");
    assert_eq!(saved.path, written.display().to_string());
    let got = page.get("file?rev=saved");
    assert_eq!(got.status, 200);
    assert_eq!(
        got.header("x-specify-path"),
        Some(written.display().to_string().as_str())
    );
    assert_eq!(got.body, std::fs::read(&written).unwrap());
}

/// The session's answers: read, and streamed to every open page (a save's change too, with a
/// `saved` event), at most four streams at once. The page cannot replace them (`PUT answers` is
/// gone, contract api 2).
#[test]
fn answers_are_read_and_streamed_with_each_save() {
    let scratch = Scratch::new("answers");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let mut events = page.events();
    let (name, start) = events.next();
    assert_eq!(name, "answers");
    let resumed = analysis_of(&scratch.input())["resume"]["answers"].clone();
    assert_eq!(
        start,
        json!({"rev": 0, "parts": {"0": resumed}, "source": "start"})
    );
    let put = page.send(
        "PUT",
        "answers",
        &json!({"rev": 0, "part": 0, "answers": {"part.material": "6061-T6"}}),
    );
    assert_eq!(put.status, 405);

    // A save that stores a decision changes the session's answers: every page hears it.
    let mut second = page.events();
    assert_eq!(second.next().1["rev"], 0);
    let saved = page.save(0, decide_coating());
    assert_eq!(
        saved.status,
        200,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    let saved = saved.json();
    assert_eq!(saved["rev"], 1);
    let written = scratch.path("work/part.specified.step");
    let stored = analysis_of(&written)["resume"]["answers"].clone();
    assert!(stored.get("part.coating").is_some(), "{stored}");
    for stream in [&mut events, &mut second] {
        let (name, change) = stream.next();
        assert_eq!(name, "answers");
        assert_eq!(
            change,
            json!({"rev": 1, "part": 0, "answers": stored, "source": "page", "drafts": {}})
        );
        let (name, told) = stream.next();
        assert_eq!(name, "saved");
        assert_eq!(
            told,
            json!({"path": saved["path"], "bytes": saved["bytes"], "rev": 1, "source": "page"})
        );
    }
    assert_eq!(page.answers(), json!({"rev": 1, "parts": {"0": stored}}));

    // Four streams at once: a fifth is refused until one closes.
    let _third = page.events();
    let _fourth = page.events();
    let fifth = page.get("events");
    assert_eq!(fifth.status, 503);
    assert_eq!(fifth.header("retry-after"), Some("1"));
    assert!(fifth.json()["error"].is_string());
}

/// A save writes what `specify-core write STEP EDITS.json` writes for the same edit list.
#[test]
fn a_save_writes_what_write_writes_for_the_same_list() {
    let scratch = Scratch::new("as-write");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let list = json!({
        "fill": {"part.general_tolerance": "ISO 2768-m", "hole.function:holes:14": "tapped:M4"},
        "none": ["part.coating"],
    });
    let saved = page.save(0, list.clone());
    assert_eq!(
        saved.status,
        200,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    let saved = saved.json();
    assert_eq!(
        saved["complete"],
        json!(saved["missing"].as_array().unwrap().is_empty())
    );

    let edits = scratch.path("edits.json");
    let out = scratch.path("written.step");
    std::fs::write(&edits, list.to_string()).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("write")
        .arg(scratch.input())
        .arg(&edits)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let report: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(
        report_without_output(&saved["report"]),
        report_without_output(&report)
    );
    let saved_path = PathBuf::from(saved["path"].as_str().unwrap());
    assert_eq!(analysis_of(&saved_path), analysis_of(&out));
    for entity in [
        "DATUM_FEATURE(",
        "GEOMETRIC_TOLERANCE",
        "DIMENSIONAL_SIZE(",
        "pmi-assist answers",
    ] {
        assert_eq!(
            occurrences(&saved_path, entity),
            occurrences(&out, entity),
            "{entity}"
        );
    }
}

#[test]
fn in_place_keeps_one_bak_per_run_and_writes_from_the_original() {
    let scratch = Scratch::new("in-place");
    let input = std::fs::read(scratch.input()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(scratch.input(), std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    let opened = Opened::start(&scratch.input(), &scratch, &["--in-place"]);
    let page = opened.page();
    assert_eq!(
        opened.config()["saveTarget"],
        scratch.input().display().to_string()
    );
    let got = page.save(
        0,
        json!({"fill": {"datum.A": [0], "part.general_tolerance": "ISO 2768-m"},
               "none": ["part.coating"]}),
    );
    assert_eq!(got.status, 200, "{}", String::from_utf8_lossy(&got.body));
    let got = got.json();
    let rev = got["rev"].as_u64().unwrap();
    assert_eq!(rev, 1);
    assert_eq!(got["path"], scratch.input().display().to_string());
    let once = std::fs::read(scratch.input()).unwrap();
    assert!(once.starts_with(b"ISO-10303-21;") && once != input);
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak")).unwrap(),
        input
    );
    assert_eq!(opened.get("file").body, once);
    let pmi = |path: &Path| {
        [
            "DATUM_FEATURE(",
            "GEOMETRIC_TOLERANCE",
            "pmi-assist answers",
        ]
        .map(|e| occurrences(path, e))
    };
    let first = pmi(&scratch.input());
    assert!(first[0] > 0, "the datum is written");
    assert!(first[2] > 0, "the decision is stored");

    // A later save replaces the file, from the file as the last save left it (an empty list
    // changes nothing, and adds nothing twice), and makes no other backup: the .bak still
    // holds the original.
    assert_eq!(opened.save(rev, json!({})).status, 200);
    assert_eq!(pmi(&scratch.input()), first);
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak")).unwrap(),
        input
    );
    assert_eq!(scratch.work(), ["part.step", "part.step.bak"]);
    // The replacement keeps the original's permissions.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(scratch.input())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640);
    }
    // A file changed on disk by someone else is not written over.
    let mut theirs = std::fs::read(scratch.input()).unwrap();
    theirs.extend_from_slice(b"\n");
    std::fs::write(scratch.input(), &theirs).unwrap();
    let refused = opened.save(rev, json!({}));
    assert_eq!(refused.status, 409);
    assert_eq!(refused.json()["rev"], rev);
    assert_eq!(std::fs::read(scratch.input()).unwrap(), theirs);
    drop(opened);

    // A new run (of the file the last one wrote, which resumes its answers) keeps the .bak and
    // backs up to .bak-2, once.
    let before = std::fs::read(scratch.input()).unwrap();
    let opened = Opened::start(&scratch.input(), &scratch, &["--in-place"]);
    assert!(
        opened.page().answers()["parts"]["0"]
            .get("part.coating")
            .is_some(),
        "the decision resumes"
    );
    assert_eq!(opened.save(0, json!({})).status, 200);
    assert_eq!(opened.save(0, json!({})).status, 200);
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak")).unwrap(),
        input
    );
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak-2")).unwrap(),
        before
    );
    assert_eq!(pmi(&scratch.input()), first);
    assert_eq!(
        scratch.work(),
        ["part.step", "part.step.bak", "part.step.bak-2"]
    );
}

/// `proj/part.step` a link to `lib/abc.step`, reached through `alias`, a link to `proj`; the
/// app opened through the links as given, the root not resolved.
#[cfg(unix)]
#[test]
fn a_linked_input_saves_beside_the_link_and_shows_the_path_as_given() {
    let scratch = Scratch::new("linked");
    for dir in ["proj", "lib"] {
        std::fs::create_dir_all(scratch.path(dir)).unwrap();
    }
    std::fs::write(scratch.path("lib/abc.step"), step_text()).unwrap();
    std::os::unix::fs::symlink(scratch.path("lib/abc.step"), scratch.path("proj/part.step"))
        .unwrap();
    std::os::unix::fs::symlink(scratch.path("proj"), scratch.path("alias")).unwrap();
    let given = scratch.given.join("alias/part.step");
    let names = |dir: &str| {
        let mut n: Vec<String> = std::fs::read_dir(scratch.path(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        n.sort();
        n
    };
    let shown = |name: &str| scratch.given.join("alias").join(name).display().to_string();

    let opened = Opened::start(&given, &scratch, &[]);
    assert_eq!(opened.config()["name"], "part.step");
    assert_eq!(opened.config()["path"], shown("part.step"));
    assert_eq!(opened.config()["saveTarget"], shown("part.specified.step"));
    let got = opened.save(0, json!({}));
    assert_eq!(got.status, 200);
    assert_eq!(got.json()["path"], shown("part.specified.step"));
    assert!(is_step(&scratch.path("proj/part.specified.step")));
    assert_eq!(
        names("lib"),
        ["abc.step"],
        "nothing in the link's target's directory"
    );
    drop(opened);

    // In place: the link's target is replaced, the link kept, the .bak beside the link.
    let opened = Opened::start(&given, &scratch, &["--in-place"]);
    assert_eq!(opened.config()["saveTarget"], shown("part.step"));
    let got = opened.save(0, decide_coating());
    assert_eq!(got.status, 200);
    assert_eq!(got.json()["path"], shown("part.step"));
    assert_eq!(opened.save(1, json!({})).status, 200);
    assert!(
        std::fs::symlink_metadata(scratch.path("proj/part.step"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link is kept"
    );
    assert!(occurrences(&scratch.path("lib/abc.step"), "pmi-assist answers") > 0);
    assert_eq!(
        std::fs::read(scratch.path("proj/part.step.bak")).unwrap(),
        step_text()
    );
    assert_eq!(names("lib"), ["abc.step"], "no temporary file left");
    assert_eq!(
        names("proj"),
        ["part.specified.step", "part.step", "part.step.bak"]
    );
}

#[test]
fn stp_and_gzipped_inputs_save_as_plain_specified_step() {
    let scratch = Scratch::new("names");
    let stp = scratch.path("work/a.stp");
    std::fs::write(&stp, step_text()).unwrap();
    let gz = scratch.path("work/b.step.gz");
    std::fs::copy(fixture(), &gz).unwrap();
    for (input, saved) in [(&stp, "a.specified.step"), (&gz, "b.specified.step")] {
        let opened = Opened::start(input, &scratch, &[]);
        let saved = scratch.path("work").join(saved);
        assert_eq!(opened.config()["saveTarget"], saved.display().to_string());
        let got = opened.save(0, json!({}));
        assert_eq!(got.status, 200);
        assert_eq!(got.json()["path"], saved.display().to_string());
        assert!(is_step(&saved), "uncompressed");
    }
    assert_eq!(
        std::fs::read(&gz).unwrap(),
        std::fs::read(fixture()).unwrap()
    );
    assert_eq!(
        scratch.work(),
        [
            "a.specified.step",
            "a.stp",
            "b.specified.step",
            "b.step.gz",
            "part.step"
        ]
    );
}

#[test]
fn in_place_on_a_gzipped_input_writes_it_gzipped() {
    let scratch = Scratch::new("in-place-gz");
    let gz = scratch.path("work/part.step.gz");
    std::fs::copy(fixture(), &gz).unwrap();
    let opened = Opened::start(&gz, &scratch, &["--in-place"]);
    let saved = opened.save(0, decide_coating());
    assert_eq!(saved.status, 200);
    let stored = std::fs::read(&gz).unwrap();
    assert_eq!(saved.json()["bytes"], stored.len());
    let text = gunzip(&stored);
    assert!(text.starts_with(b"ISO-10303-21;"));
    assert!(String::from_utf8_lossy(&text).contains("pmi-assist answers"));
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.gz.bak")).unwrap(),
        std::fs::read(fixture()).unwrap()
    );
}

/// `open --diff OLD NEW`: the comparison `specify-core diff` makes, OLD and NEW served, and
/// the session NEW's own, as `open NEW` starts it: nothing is carried (ADR P11).
#[test]
fn a_diff_serves_both_files_and_starts_from_the_new_file() {
    let scratch = Scratch::new("diff");
    let old = scratch.path("work/plate.old.step.gz");
    let new = scratch.path("work/plate.new.step.gz");
    std::fs::copy(revision("hole_moved.old.step.gz"), &old).unwrap();
    std::fs::copy(revision("hole_moved.new.step.gz"), &new).unwrap();
    let opened = Opened::diff(&old, &new, &scratch, &[]);

    // What `specify-core diff` writes for the same pair.
    let dir = scratch.path("diffed");
    let run = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .args([&old, &new])
        .arg("-o")
        .arg(&dir)
        .arg("--pmi-scenes")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let read = |name: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    };

    let config = opened.config();
    assert_eq!(config["mode"], "diff");
    assert_eq!(config["name"], "plate.new.step.gz");
    assert_eq!(config["path"], new.display().to_string());
    assert_eq!(config["oldName"], "plate.old.step.gz");
    assert_eq!(config["oldPath"], old.display().to_string());
    assert_eq!(config["readOnly"], false);
    assert_eq!(
        config["saveTarget"],
        scratch
            .path("work/plate.new.specified.step")
            .display()
            .to_string()
    );
    assert_eq!(
        opened.get("file?rev=old").body,
        std::fs::read(&old).unwrap()
    );
    assert_eq!(
        opened.get("file?rev=new").body,
        std::fs::read(&new).unwrap()
    );
    assert_eq!(opened.get("file").body, std::fs::read(&new).unwrap());
    assert_eq!(opened.get("file?rev=other").status, 404);
    assert_eq!(
        opened.get("file?rev=saved").status,
        404,
        "nothing saved yet"
    );
    let report = opened.get("diff");
    assert_eq!(report.status, 200);
    // As WASM `diff()`: `diff`'s report.json, the report as Markdown, and the view to draw.
    let served = report.json();
    assert_eq!(served["report"], read("report.json"));
    assert!(served["markdown"].is_string() && served["view"]["new"].is_object());
    assert!(!dir.join("answers.json").exists());
    // NEW stores no answers, and none are carried to it.
    assert_eq!(opened.page().answers(), json!({"rev": 0, "parts": {}}));
    // Save writes NEW, beside it.
    let saved = opened.save(0, json!({}));
    assert_eq!(saved.status, 200);
    let written = scratch.path("work/plate.new.specified.step");
    assert_eq!(saved.json()["path"], written.display().to_string());
    // `rev=saved` serves what it wrote, as in the other modes.
    let got = opened.get("file?rev=saved");
    assert_eq!(got.status, 200);
    assert_eq!(got.body, std::fs::read(&written).unwrap());
    assert_eq!(
        got.header("x-specify-path"),
        Some(written.display().to_string().as_str())
    );
}

#[test]
fn the_close_delay_outlasts_the_heartbeat() {
    assert!(open::CLOSE_DELAY > open::HEARTBEAT);
    assert_eq!(open::HEARTBEAT, Duration::from_secs(30));
    assert_eq!(open::CLOSE_DELAY, Duration::from_secs(90));
    assert_eq!(open::IDLE, Duration::from_secs(30 * 60));
    assert_eq!(open::KEEP_ALIVE, Duration::from_secs(15));
    assert_eq!(open::MAX_STREAMS, 4);
}

#[test]
fn closed_stops_the_server_unless_a_page_comes_back_or_is_still_open() {
    let scratch = Scratch::new("closed");
    let delay = Duration::from_millis(1500);
    let beat = Duration::from_millis(300);
    let served = Served::start(&scratch, delay, Duration::from_secs(600));
    let own = format!("http://127.0.0.1:{}", served.port);

    // Only from the app's own page.
    assert_eq!(
        served.post("closed", Some("https://example.com")).status,
        403
    );
    assert_eq!(served.post("closed", None).status, 403);

    // A reload: closed, then the page asks again.
    assert_eq!(served.closed().status, 204);
    assert_eq!(served.page().get("").status, 200);
    std::thread::sleep(delay + Duration::from_millis(500));
    assert!(!served.thread.is_finished(), "a reload keeps it running");

    // One tab closed, another still open: its heartbeats keep the server past the delay.
    assert_eq!(served.post("closed", Some(&own)).status, 204);
    served.heartbeats(beat, delay * 2);

    // An open event stream keeps it too, and its end counts as the last request.
    let events = served.page().events();
    assert_eq!(served.closed().status, 204);
    std::thread::sleep(delay + Duration::from_millis(500));
    assert!(
        !served.thread.is_finished(),
        "an open stream keeps it running"
    );
    drop(events);

    // The last tab closed: it stops once no request has come for the delay.
    assert_eq!(served.closed().status, 204);
    assert_eq!(served.stopped(delay + Duration::from_secs(5)), ["Closed."]);
}

#[test]
fn heartbeats_keep_an_idle_server_running() {
    let scratch = Scratch::new("heartbeat");
    // An idle time well above the beat, so a beat delayed on a loaded machine (other tests
    // save in parallel) does not look like silence.
    let idle = Duration::from_millis(3000);
    let served = Served::start(&scratch, Duration::from_secs(600), idle);
    served.heartbeats(Duration::from_millis(200), idle * 3);
    let notices = served.stopped(idle + Duration::from_secs(5));
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].starts_with("Stopped: no request for"),
        "{notices:?}"
    );
}

#[test]
fn a_server_without_requests_stops_after_its_idle_time() {
    let scratch = Scratch::new("idle");
    let app = App::new(
        session_of(&scratch.input()),
        scratch.assets(),
        &Options::default(),
    )
    .unwrap();
    let port = app.port();
    let mut notices = Vec::new();
    app.serve(Duration::from_secs(3), Duration::from_millis(300), |n| {
        notices.push(n.to_string())
    })
    .unwrap();
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].starts_with("Stopped: no request for"),
        "{notices:?}"
    );
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the port is closed"
    );
}

#[test]
fn bad_arguments_and_names_that_are_not_step_are_refused_up_front() {
    let scratch = Scratch::new("args");
    let stl = scratch.path("work/part.stl");
    std::fs::write(&stl, b"solid").unwrap();
    let input = scratch.input();

    for (inputs, extra) in [
        (vec![input.as_path()], vec!["--read-only", "--in-place"]),
        (vec![input.as_path(), input.as_path()], vec!["--read-only"]),
    ] {
        let conflict = command(&inputs, &scratch, &extra).output().unwrap();
        assert_eq!(conflict.status.code(), Some(2), "{extra:?}");
        assert!(conflict.stdout.is_empty());
        let stderr = String::from_utf8(conflict.stderr).unwrap();
        assert!(stderr.contains("cannot be used with"), "{stderr}");
    }
    // There is no --answers, with or without --diff (ADR P11).
    let answers = command(&[input.as_path()], &scratch, &["--answers", "a.json"])
        .output()
        .unwrap();
    assert_eq!(answers.status.code(), Some(2));

    for inputs in [vec![stl.as_path()], vec![stl.as_path(), input.as_path()]] {
        let out = command(&inputs, &scratch, &[]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.starts_with("cannot open: "), "{stderr}");
        assert!(stderr.contains("is not a STEP file"), "{stderr}");
    }

    // The environment names the app as well as --app does.
    let mut from_env = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("open")
        .arg(scratch.input())
        .arg("--no-browser")
        .env("SPECIFY_APP_DIR", scratch.path("app"))
        .env("SPECIFY_WASM_DIR", scratch.path("pkg"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(from_env.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let _ = from_env.kill();
    let _ = from_env.wait();
    assert!(
        line.starts_with("Open part.step: http://127.0.0.1:"),
        "{line}"
    );

    let not_app = command(
        &[input.as_path()],
        &scratch,
        &["--app", scratch.path("pkg").to_str().unwrap()],
    )
    .output()
    .unwrap();
    assert_eq!(not_app.status.code(), Some(2));
    assert!(
        String::from_utf8(not_app.stderr)
            .unwrap()
            .contains("not a directory holding index.html")
    );

    let missing = command(&[&scratch.path("work/missing.step")], &scratch, &[])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .starts_with("error: ")
    );

    let help = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(["open", "-h"])
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0));
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(
        help.starts_with("Open a STEP file in Specify in your browser"),
        "{help}"
    );
    assert!(help.contains("--diff <OLD> <NEW>"), "{help}");
    assert!(
        !help.contains("--site") && !help.contains("--editor"),
        "{help}"
    );
    assert!(!help.contains("draftwright.io"), "{help}");
}

/// The built-in app and engine (ADR P12): the test build has tests/fixtures/bundle's built in.
fn built_in(rel: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/bundle")
            .join(rel),
    )
    .unwrap()
}

/// `open` with no `--app` or `--wasm` overrides, no user cache and no way to the network.
fn offline_command(scratch: &Scratch, extra: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_specify-core"));
    let home = scratch.path("home");
    std::fs::create_dir_all(&home).unwrap();
    c.arg("open")
        .arg(scratch.input())
        .arg("--no-browser")
        .args(extra)
        .env_remove("SPECIFY_APP_DIR")
        .env_remove("SPECIFY_WASM_DIR")
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("LOCALAPPDATA", &home);
    // A proxy that refuses every connection: a download would fail.
    for proxy in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
    ] {
        c.env(proxy, "http://127.0.0.1:9");
    }
    c.env_remove("NO_PROXY").env_remove("no_proxy");
    c
}

#[test]
fn open_serves_the_built_in_app_and_engine_with_nothing_downloaded() {
    let scratch = Scratch::new("built-in");
    let opened = Opened::from_command(
        &scratch.input(),
        offline_command(&scratch, &["--read-only"]),
    );

    let index = opened.get("");
    assert_eq!(index.status, 200);
    assert_eq!(index.body, built_in("app/index.html"));
    assert_eq!(
        index.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(index.header("cache-control"), Some("no-store"));
    assert!(
        index
            .header("content-security-policy")
            .unwrap()
            .contains("'wasm-unsafe-eval'")
    );
    for (rel, content_type, file) in [
        ("index.html", "text/html; charset=utf-8", "app/index.html"),
        (
            "assets/main.js",
            "text/javascript; charset=utf-8",
            "app/assets/main.js",
        ),
        (
            "specify-ui-app.json",
            "application/json",
            "app/specify-ui-app.json",
        ),
        (
            "wasm/specify_core_wasm.js",
            "text/javascript; charset=utf-8",
            "wasm/specify_core_wasm.js",
        ),
        (
            "wasm/specify_core_wasm_bg.wasm",
            "application/wasm",
            "wasm/specify_core_wasm_bg.wasm",
        ),
        // Percent-encoded segments name the same file.
        (
            "assets/%6Dain.js",
            "text/javascript; charset=utf-8",
            "app/assets/main.js",
        ),
    ] {
        let got = opened.get(rel);
        assert_eq!(got.status, 200, "{rel}");
        assert_eq!(got.header("content-type"), Some(content_type), "{rel}");
        assert_eq!(got.body, built_in(file), "{rel}");
    }
    // Only the built-in files: no directory, no way out, nothing else.
    for rel in [
        "assets",
        "assets/",
        "assets/../index.html",
        "assets/%2E%2E/index.html",
        "./index.html",
        "assets//main.js",
        "wasm",
        "wasm/",
        "wasm/../index.html",
        "wasm/missing.wasm",
        "missing.html",
        "assets%2Fmain.js",
    ] {
        assert_eq!(opened.get(rel).status, 404, "{rel}");
    }

    let config = opened.config();
    assert_eq!(config["wasmUrl"], "wasm/specify_core_wasm.js");
    assert_eq!(config["api"], open::API_VERSION);
    assert_eq!(config["readOnly"], true);
    // Nothing was written outside the scratch input's directory.
    assert_eq!(
        std::fs::read_dir(scratch.path("home")).unwrap().count(),
        0,
        "nothing cached"
    );
}

/// `--app` overrides the built-in app; without `--wasm` its own `wasm/` serves the engine, and
/// without one the built-in engine does. `--wasm` overrides both.
#[test]
fn app_and_wasm_override_the_built_in_ones() {
    let scratch = Scratch::new("override");
    let app = scratch.path("app");
    let app = app.to_str().unwrap();
    let pkg = scratch.path("pkg");
    let pkg = pkg.to_str().unwrap();

    let opened = Opened::from_command(&scratch.input(), offline_command(&scratch, &["--app", app]));
    assert_eq!(
        opened.get("").body,
        b"<!doctype html><title>stub app</title>"
    );
    assert_eq!(opened.get("assets/main.js").status, 404);
    let engine = opened.get("wasm/specify_core_wasm.js");
    assert_eq!(engine.status, 200);
    assert_eq!(engine.body, built_in("wasm/specify_core_wasm.js"));
    drop(opened);

    let opened = Opened::from_command(
        &scratch.input(),
        offline_command(&scratch, &["--app", app, "--wasm", pkg]),
    );
    assert_eq!(
        opened.get("").body,
        b"<!doctype html><title>stub app</title>"
    );
    assert_eq!(
        opened.get("wasm/specify_core_wasm.js").body,
        b"export default 1;"
    );
    drop(opened);

    // An app's own wasm/ directory is its engine.
    std::fs::create_dir(scratch.path("app/wasm")).unwrap();
    std::fs::write(scratch.path("app/wasm/specify_core_wasm.js"), b"own").unwrap();
    let opened = Opened::from_command(&scratch.input(), offline_command(&scratch, &["--app", app]));
    assert_eq!(opened.get("wasm/specify_core_wasm.js").body, b"own");
    drop(opened);

    // --wasm alone: the built-in app on that engine.
    let opened = Opened::from_command(
        &scratch.input(),
        offline_command(&scratch, &["--wasm", pkg]),
    );
    assert_eq!(opened.get("").body, built_in("app/index.html"));
    assert_eq!(
        opened.get("wasm/specify_core_wasm.js").body,
        b"export default 1;"
    );
    drop(opened);

    // The environment overrides as the flags do.
    let mut from_env = offline_command(&scratch, &[]);
    from_env.env("SPECIFY_APP_DIR", app);
    let opened = Opened::from_command(&scratch.input(), from_env);
    assert_eq!(
        opened.get("").body,
        b"<!doctype html><title>stub app</title>"
    );
}

/// An `--app` whose manifest speaks another session API is refused before anything is served;
/// one of this API is served.
#[test]
fn an_app_of_another_session_api_is_refused() {
    let scratch = Scratch::new("app-api");
    let app = scratch.path("app");
    let manifest =
        |api: u32| json!({"name": "specify-ui-app", "version": "9.9.9", "api": api}).to_string();
    std::fs::write(
        app.join("specify-ui-app.json"),
        manifest(open::API_VERSION + 1),
    )
    .unwrap();
    let out = offline_command(&scratch, &["--app", app.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("cannot open: "), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "this app speaks session API {}, specify-core {} speaks {}",
            open::API_VERSION + 1,
            env!("CARGO_PKG_VERSION"),
            open::API_VERSION
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains("pass --app DIR / set SPECIFY_APP_DIR"),
        "{stderr}"
    );

    std::fs::write(app.join("specify-ui-app.json"), manifest(open::API_VERSION)).unwrap();
    let opened = Opened::from_command(
        &scratch.input(),
        offline_command(&scratch, &["--app", app.to_str().unwrap()]),
    );
    assert_eq!(opened.get("").status, 200);
}

#[test]
fn step_names_are_step_or_stp_or_either_gzipped() {
    for name in ["a.step", "a.STP", "a.Step.GZ", "a.stp.gz"] {
        assert!(open::is_step_name(name), "{name}");
    }
    for name in [
        "a.stl",
        "a.step.zip",
        "a.gz",
        "step",
        "a.steps",
        "a.step.gz.txt",
    ] {
        assert!(!open::is_step_name(name), "{name}");
    }
}

/// Saves racing each other at one rev: exactly one is written; the others find the session
/// moved on (409) and write nothing, so the file holds exactly the winner's list.
#[test]
fn racing_saves_at_one_rev_write_exactly_one() {
    let scratch = Scratch::new("race");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let ids = [
        "part.coating",
        "part.heat_treatment",
        "part.surface_finish",
        "part.edges",
    ];
    let racers: Vec<_> = ids
        .iter()
        .map(|id| {
            let page = page.clone();
            let id = id.to_string();
            // Each a list the file takes on its own: a decision of none where none is an
            // option, else a fill of one of the question's options.
            let list = match id.as_str() {
                "part.surface_finish" => json!({"fill": {(id.clone()): "Ra 3.2"}}),
                "part.edges" => json!({"fill": {(id.clone()): "Deburr only"}}),
                _ => json!({"none": [id]}),
            };
            std::thread::spawn(move || (id.clone(), page.save(0, list).status))
        })
        .collect();
    let results: Vec<(String, u16)> = racers.into_iter().map(|r| r.join().unwrap()).collect();
    let won: Vec<&String> = results
        .iter()
        .filter(|(_, status)| *status == 200)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(won.len(), 1, "{results:?}");
    assert!(
        results.iter().all(|(_, s)| *s == 200 || *s == 409),
        "{results:?}"
    );
    assert_eq!(page.rev(), 1);
    let table = analysis_of(&scratch.path("work/part.specified.step"))["table"].clone();
    for id in ids {
        let row = table["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap();
        assert_eq!(row["state"] != "gap", id == won[0].as_str(), "{id}: {row}");
    }
    // One more, certainly stale.
    assert_eq!(page.save(0, json!({})).status, 409);
}

/// An event stream that stops reading is dropped once it is more than `MAX_BEHIND` events
/// behind (its receiver ends after what it was sent); the others carry on.
#[test]
fn a_stream_too_far_behind_is_dropped_and_the_others_carry_on() {
    let scratch = Scratch::new("behind");
    let shared = session_of(&scratch.input());
    let (stuck, reading) = {
        let mut s = shared.lock().unwrap();
        (s.subscribe(), s.subscribe())
    };
    assert!(reading.try_recv().is_ok(), "the start event");
    let limit = specify_core::session::MAX_BEHIND;
    for i in 0..=limit {
        let material = if i % 2 == 0 { "6061-T6" } else { "6082-T6" };
        let answers: Map<String, Value> =
            serde_json::from_value(json!({ "part.material": material })).unwrap();
        assert!(shared.lock().unwrap().set(0, answers, "page").is_some());
        assert!(reading.try_recv().is_ok(), "change {i} reaches the reader");
    }
    // The stuck one got the start and `limit` changes, then was dropped.
    assert_eq!(stuck.try_iter().count(), 1 + limit);
    assert!(matches!(
        stuck.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    let answers: Map<String, Value> =
        serde_json::from_value(json!({"part.material": "7075-T6"})).unwrap();
    shared.lock().unwrap().set(0, answers, "page");
    assert!(reading.try_recv().is_ok(), "the reader carries on");
}

/// ADR P9: a suggestion is written only when the list names it. The one accepted is stated
/// in the file; the others stay gaps.
#[test]
fn only_the_suggestions_named_are_written() {
    let scratch = Scratch::new("approved");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let gaps: Vec<String> = analysis_of(&scratch.input())["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["state"] == "gap" && r["open"] == true)
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        gaps.contains(&"part.general_tolerance".to_string()),
        "{gaps:?}"
    );
    let saved = page.save(0, json!({"accept_suggestions": ["part.general_tolerance"]}));
    assert_eq!(
        saved.status,
        200,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    let written = PathBuf::from(saved.json()["path"].as_str().unwrap());
    let rows = analysis_of(&written)["table"]["rows"].clone();
    let state = |id: &str| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .map(|r| r["state"].clone())
            .unwrap()
    };
    assert_eq!(state("part.general_tolerance"), "stated");
    for other in gaps.iter().filter(|g| *g != "part.general_tolerance") {
        assert_eq!(state(other), "gap", "{other} is not written");
    }
}

/// PMI counts that a write keeps or loses, from the STEP text.
fn pmi_counts(path: &Path) -> [usize; 3] {
    ["DATUM_FEATURE(", "GEOMETRIC_TOLERANCE", "DIMENSIONAL_SIZE("].map(|e| occurrences(path, e))
}

/// A file saved in the boolean era (every default written, the stored answers only metadata)
/// keeps its PMI when a session of it saves with nothing to change: a part whose answers are
/// only `_`-keys is not written, and a file with nothing to write is saved as it was.
#[test]
fn a_part_with_only_metadata_answers_keeps_its_pmi() {
    let scratch = Scratch::new("metadata-only");
    let plate = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step");
    let analysis = analysis_of(&plate);
    let loaded = specify_core::part::load(&plate, None).unwrap();
    let given: Map<String, Value> =
        serde_json::from_value(json!({"part.material": "6061-T6"})).unwrap();
    // Only the metadata, as a boolean-era save with every default taken stores it.
    let mut stored = Map::new();
    specify_core::answer_file::attach(&loaded, &mut stored, &given, std::iter::empty()).unwrap();
    assert!(!stored.is_empty());
    let intent = specify_core::api::apply(&analysis, &given, true).unwrap();
    let old = scratch.path("work/old.step");
    specify_core::api::write_file(&plate, &intent, &old, Some(&stored)).unwrap();
    let before = pmi_counts(&old);
    assert!(
        before.iter().sum::<usize>() > 0,
        "the defaults' PMI is written"
    );
    let resumed = analysis_of(&old);
    assert!(
        resumed["resume"]["answers"]
            .as_object()
            .unwrap()
            .keys()
            .all(|k| k.starts_with('_')),
        "{}",
        resumed["resume"]
    );

    let shared = session_of(&old);
    let saved = specify_core::session::save_edits(&shared, None, &Default::default(), "page")
        .unwrap_or_else(|e| panic!("{e}"));
    let path = PathBuf::from(&saved.path);
    assert_eq!(pmi_counts(&path), before, "nothing dropped");
    assert_eq!(saved.report["written"], json!([]));
}

/// ADR P11: leaving something out removes nothing. A save of an empty list keeps the
/// decisions the file stores (and every byte); removing one takes an explicit edit.
#[test]
fn an_empty_list_removes_no_stored_decision() {
    let scratch = Scratch::new("cleared");
    let first = Opened::start(&scratch.input(), &scratch, &[]);
    let stored = first.page().save(0, decide_coating());
    assert_eq!(stored.status, 200);
    let stored = PathBuf::from(stored.json()["path"].as_str().unwrap());
    drop(first);
    let opened = Opened::start(&stored, &scratch, &[]);
    let page = opened.page();
    let resumed = page.answers()["parts"]["0"].clone();
    assert!(resumed.get("part.coating").is_some(), "{resumed}");
    let saved = page.save(0, json!({}));
    assert_eq!(
        saved.status,
        200,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    let saved = saved.json();
    assert_eq!(saved["report"]["unchanged"], true);
    let written = PathBuf::from(saved["path"].as_str().unwrap());
    assert_eq!(
        std::fs::read(&written).unwrap(),
        std::fs::read(&stored).unwrap()
    );
    drop(opened);
    let reopened = Opened::start(&written, &scratch, &[]);
    let parts = reopened.page().answers()["parts"].clone();
    assert_eq!(parts["0"], resumed, "{parts}");
}

/// ADR P11: a page Save that changes nothing (a fill equal to what the file states) leaves the
/// file byte-identical, beside it and over it in place: nothing is rewritten or added (the old
/// answers save wrote it again through `writer::write_bytes_with`, adding an empty stored-answers
/// record and a description).
#[test]
fn a_no_change_save_leaves_the_file_byte_identical() {
    for in_place in [false, true] {
        let scratch = Scratch::new(if in_place {
            "same-in-place"
        } else {
            "same-copy"
        });
        // A file Specify wrote, stating the material (nothing stored beside it).
        let first = Opened::start(&scratch.input(), &scratch, &[]);
        let steel = json!({"fill": {"part.material": "steel"}});
        let stored = first.page().save(0, steel.clone());
        assert_eq!(stored.status, 200);
        let input = PathBuf::from(stored.json()["path"].as_str().unwrap());
        drop(first);
        let before = std::fs::read(&input).unwrap();
        let extra: &[&str] = if in_place { &["--in-place"] } else { &[] };
        let opened = Opened::start(&input, &scratch, extra);
        // The material again, as the file states it: a fill that changes nothing (the old
        // answers save of the same answer wrote the file again, with an empty stored record).
        let saved = opened.page().save(0, steel);
        assert_eq!(
            saved.status,
            200,
            "{}",
            String::from_utf8_lossy(&saved.body)
        );
        let saved = saved.json();
        assert_eq!(saved["report"]["unchanged"], true, "{saved}");
        let written = PathBuf::from(saved["path"].as_str().unwrap());
        assert_eq!(written == input, in_place, "{saved}");
        assert!(
            std::fs::read(&written).unwrap() == before,
            "in place {in_place}: the saved file differs from the input"
        );
    }
}

/// Sets part 0's answers, as an agent's `answer` would, between a save's applying its list and
/// its writing the file.
fn answered_meanwhile(shared: &specify_core::session::Shared) {
    let mut s = specify_core::session::lock(shared);
    let mut answers = s.answers(0).cloned().unwrap_or_default();
    answers.insert("part.heat_treatment".into(), json!("None"));
    s.set(0, answers, "mcp");
}

/// An agent changes the answers just before commit. Even a save without a requested revision
/// must refuse as stale and keep the newer answers.
#[test]
fn a_session_changed_during_a_save_without_a_rev_is_refused() {
    let scratch = Scratch::new("changed-meanwhile");
    let shared = session_of(&scratch.input());
    specify_core::session::lock(&shared).between_locks = Some(answered_meanwhile);
    let list = specify_core::edits::parse_value(decide_coating()).unwrap();
    let refused = specify_core::session::save_edits(&shared, None, &list, "mcp");
    assert!(
        matches!(refused, Err(specify_core::session::SaveError::Stale(1))),
        "{refused:?}"
    );
    assert_eq!(scratch.work(), ["part.step"], "nothing written");
    let s = specify_core::session::lock(&shared);
    assert_eq!(s.answers(0).unwrap()["part.heat_treatment"], "None");
}

/// Exercise the old gap after the final check. Try the same lock MCP takes to publish an
/// answer. On the old commit path the answer can enter here and the save erases it.
fn answer_at_commit(shared: &specify_core::session::Shared) {
    match shared.try_lock() {
        Err(std::sync::TryLockError::WouldBlock) => {}
        Ok(mut session) => {
            let mut answers = session.answers(0).cloned().unwrap_or_default();
            answers.insert("part.heat_treatment".into(), json!("None"));
            session.set(0, answers, "mcp");
        }
        Err(err) => panic!("{err}"),
    }
}

#[test]
fn an_answer_cannot_enter_after_the_final_revision_check() {
    let scratch = Scratch::new("commit-boundary");
    let file = scratch.input();
    let shared = Arc::new(Mutex::new(
        Session::open(OpenedFile::read(&file).unwrap(), true).unwrap(),
    ));
    specify_core::session::lock(&shared).commit_boundary = Some(answer_at_commit);
    let list = specify_core::edits::parse_value(decide_coating()).unwrap();
    let saved = specify_core::session::save_edits(&shared, Some(0), &list, "page").unwrap();
    assert_eq!(saved.rev, 1, "an answer slipped in after the final check");
    assert!(scratch.path("work/part.step.bak").exists());
    // A real update immediately after publication can take the lock and remains in the session.
    answer_at_commit(&shared);
    let s = specify_core::session::lock(&shared);
    assert_eq!(s.rev(), 2);
    assert_eq!(s.answers(0).unwrap()["part.heat_treatment"], "None");
}

#[test]
fn save_edits_commits_general_measurements_at_the_requested_revision() {
    let scratch = Scratch::new("general-measurements");
    let input = scratch.input();
    std::fs::write(&input, include_bytes!("fixtures/spindle.step")).unwrap();
    let opened = Opened::start(&input, &scratch, &[]);
    let length = "length:planes:8-10";
    let location = "location:holes:10-7";
    let declined = "length:planes:4-10";
    let saved = opened.save(
        0,
        json!({
            "accept_suggestions": [length, location],
            "none": [declined],
        }),
    );
    assert_eq!(
        saved.status,
        200,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    let saved = saved.json();
    assert_eq!(saved["rev"], 1);
    let path = Path::new(saved["path"].as_str().unwrap());
    let after = specify_core::api::analyse_file(path, None).unwrap();
    let state = |id| {
        after["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()["state"]
            .as_str()
            .unwrap()
    };
    assert_eq!(state(length), "stated");
    assert_eq!(state(location), "stated");
    assert_eq!(state(declined), "decided_none");
    assert_eq!(opened.save(0, json!({})).status, 409);
}

#[test]
fn page_saves_part_lists_as_one_assembly_revision() {
    let scratch = Scratch::new("assembly-edits");
    let input = scratch.input();
    let before = include_bytes!("fixtures/parts/assembly.step");
    std::fs::write(&input, before).unwrap();
    let opened = Opened::start(&input, &scratch, &["--in-place"]);
    let page = opened.page();
    let parts = specify_core::part::parts(&input).unwrap();
    let rev = page.rev();
    let plate = json!({"part": 0, "edits": {"fill": {"part.material": "Steel"}}});
    let pin = json!({"part": 1, "edits": {"fill": {"part.material": "Brass"}}});

    let bad = json!({"part": 1, "edits": {"fill": {"unknown.pin": "Brass"}}});
    let refused = page.send(
        "POST",
        "save",
        &json!({"rev": rev, "partEdits": [plate, bad]}),
    );
    assert_eq!(refused.status, 422, "{}", refused.json());
    let failure = refused.json()["failures"][0]["entry"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        failure.contains(&parts[1].name) && failure.contains("unknown.pin"),
        "{failure}"
    );
    assert_eq!(page.rev(), rev);
    assert_eq!(std::fs::read(&input).unwrap(), before);
    assert_eq!(scratch.work(), ["part.step"], "no backup on refusal");

    for malformed in [
        json!({"rev": rev, "partEdits": []}),
        json!({"rev": rev, "edits": {}, "partEdits": [plate]}),
        json!({"rev": rev, "partEdits": [plate, plate]}),
        json!({"rev": rev, "partEdits": [{"part": -1, "edits": {}}]}),
        json!({"rev": rev, "partEdits": [{"part": 1, "edits": {"fill": []}}]}),
    ] {
        assert_eq!(page.send("POST", "save", &malformed).status, 400);
    }
    let saved = page.send(
        "POST",
        "save",
        &json!({"rev": rev, "partEdits": [plate, pin]}),
    );
    assert_eq!(saved.status, 200, "{}", saved.json());
    let saved = saved.json();
    assert_eq!(saved["rev"], rev + 1);
    assert_eq!(saved["report"]["unchanged"], false);
    assert_eq!(saved["report"]["parts"][1]["name"], parts[1].name);
    for (part, material) in [(0, "Steel"), (1, "Brass")] {
        let analysis = specify_core::api::analyse_file(&input, Some(part)).unwrap();
        let rows = analysis["table"]["rows"].as_array().unwrap();
        let row = rows
            .iter()
            .find(|row| row["id"] == "part.material")
            .unwrap();
        assert_eq!(row["value"], material);
    }
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak")).unwrap(),
        before
    );
    assert_eq!(
        page.send("POST", "save", &json!({"rev": rev, "partEdits": [pin]}))
            .status,
        409
    );
}

/// The HTTP outcome of two concurrent writes based on the same revision, through each save form.
#[test]
fn simultaneous_page_saves_at_one_revision_have_one_winner() {
    for format in ["whole", "part", "batch"] {
        let scratch = Scratch::new(&format!("simultaneous-{format}"));
        let input = scratch.input();
        if format != "whole" {
            std::fs::write(&input, include_bytes!("fixtures/parts/assembly.step")).unwrap();
        }
        let opened = Opened::start(&input, &scratch, &["--in-place"]);
        let page = opened.page();
        let rev = page.rev();
        let gate = std::sync::Barrier::new(2);
        let send = |material| {
            let edits = json!({"fill": {"part.coating": material}});
            let body = match format {
                "whole" => json!({"rev": rev, "edits": edits}),
                "part" => json!({"rev": rev, "partEdits": [{"part": 0, "edits": edits}]}),
                _ => json!({"rev": rev, "partEdits": [
                    {"part": 0, "edits": edits},
                    {"part": 1, "edits": {"fill": {"part.coating": material}}}
                ]}),
            };
            gate.wait();
            page.send("POST", "save", &body)
        };
        let replies = std::thread::scope(|threads| {
            let first = threads.spawn(|| send("Zinc"));
            let second = send("Anodised");
            [first.join().unwrap(), second]
        });
        let mut codes = replies.each_ref().map(|reply| reply.status);
        codes.sort_unstable();
        assert_eq!(codes, [200, 409], "{format}");
        assert_eq!(page.rev(), rev + 1);
        let winner = if replies[0].status == 200 {
            "Zinc"
        } else {
            "Anodised"
        };
        let selection = if format == "whole" {
            vec![None]
        } else if format == "part" {
            vec![Some(0)]
        } else {
            vec![Some(0), Some(1)]
        };
        for part in selection {
            let data = specify_core::api::analyse_file(&input, part).unwrap();
            let coating = data["table"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == "part.coating")
                .unwrap();
            assert_eq!(coating["value"], winner);
        }
    }
}
