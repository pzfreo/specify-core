//! `specify-pmi`: the command line (port of `cli.py`). [`main`] runs it on its arguments: the
//! binary (`main.rs`) and the Python package's `specify-pmi` console script both call it.
//!
//! ```text
//! specify-pmi parts STEP                        the file's parts, as JSON
//! specify-pmi bom STEP                          the assembly BOM, as JSON
//! specify-pmi analyse STEP -o ANALYSIS [--part K]
//! specify-pmi mesh STEP -o MESH [--part K] [--max-deviation-mm MM]
//! specify-pmi questions ANALYSIS [--answers ANSWERS] [--open]
//! specify-pmi write STEP EDITS.json -o OUT [--part K]
//!                    apply an edit list in place, all or nothing (writer/apply.rs)
//! specify-pmi write STEP EDITS.json --check
//!                    validate an edit list against the file (edits.rs); writes nothing
//! specify-pmi diff OLD NEW -o DIR [--part K] [--title TITLE] [--viewer URL] [--pmi-scenes]
//!                    what changed between two revisions of a part and the requirements
//!                    their PMI states (revision.rs), or of an assembly, part by part
//!                    (revision/assembly.rs)
//! specify-pmi serve                             JSON-line requests on stdin (serve.rs)
//! specify-pmi mcp [--http [--port N]] [--allow DIR]...
//!                    a Model Context Protocol server on stdio or local HTTP (mcp.rs)
//! specify-pmi open STEP [--read-only | --in-place] [--port N] [--no-browser]
//!                    [--app DIR] [--wasm DIR]
//! specify-pmi open --diff OLD NEW [--in-place] [--port N] [--no-browser]
//!                    [--app DIR] [--wasm DIR]
//!                    the file (or NEW, compared with OLD) opened in the Specify app, served
//!                    on 127.0.0.1, its answers held by this command (session.rs) and saved
//!                    back to disk (open.rs)
//! specify-pmi completions SHELL                 a shell completion script (clap_complete)
//! ```
//!
//! Arguments are parsed by clap; help and usage errors are clap's (exit 0 and exit 2). A long
//! option may be shortened to any prefix only it has, a repeated option's last value wins and
//! `-h` gives a command's full help, as specify-core's argparse parser did.
//!
//! Outputs, exit codes and messages are specify-pmi's, so a host can run either engine:
//! `analyse` writes the analysis (Python's `json.dumps(indent=1)` text, non-ASCII escaped, so
//! the files compare byte for byte) and prints `{faces, features, existing_pmi}`; `mesh` writes
//! compact JSON and prints `{faces, triangles}`, warning on stderr of faces that did not
//! triangulate; `parts`, `questions` and `write` print indented JSON with non-ASCII kept. Exit 2
//! with `incomplete: ...`, `invalid answer: ...` or `cannot write: ...` on stderr (in an
//! assembly prefixed `part K: `), or with clap's usage error for bad arguments; exit 1 with
//! `write failed, OUT left as it was: ...` when a write does not verify.
//!
//! Where Python fails with an uncaught exception (a file that cannot be read or loaded, an
//! output that cannot be written) it exits 1 with a traceback; this exits 1 with `error:` and
//! the message on stderr. Python's second name for the program, `pmi-assist`, is an alias of the
//! Python package's entry point and has no counterpart for a Rust binary.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{ArgAction, CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::api::{self, Error};
use crate::part;
use crate::pyjson;
use crate::rules;
use crate::serve;
use serde_json::{Map, Value, json};

const PROG: &str = "specify-pmi";

// The command line. `-h` and `--help` both give the full help (`long_about`), as argparse's
// one help did; it is listed last, as clap lists its own.
#[derive(Parser)]
#[command(
    name = PROG,
    disable_help_flag = true,
    disable_help_subcommand = true,
    subcommand_required = true,
    infer_long_args = true,
    args_override_self = true
)]
struct Cli {
    #[arg(
        short = 'h',
        long = "help",
        action = ArgAction::HelpLong,
        global = true,
        display_order = 1000,
        help = "Print help"
    )]
    help: Option<bool>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// list an assembly's parts
    Parts { step: PathBuf },
    /// recognise features and read existing PMI
    Analyse {
        step: PathBuf,
        /// analysis JSON (`-` for stdout; the summary then goes to stderr)
        #[arg(short, long)]
        output: PathBuf,
        /// which part of an assembly (see parts)
        #[arg(long, value_parser = part_number)]
        part: Option<i64>,
    },
    /// answer JSON-line requests on stdin (for a host process)
    Serve,
    /// a Model Context Protocol server for an LLM client (stdio, or local HTTP)
    #[command(long_about = "\
A Model Context Protocol server (revisions 2026-07-28 and 2025-11-25) for an LLM client such
as Claude Code or Claude Desktop: tools to open a part, answer its questions, review and
write its PMI. On stdio by default; with --http, Streamable HTTP on 127.0.0.1 with a random
bearer token printed at start. Open documents and their answers are held in memory and lost
when the server stops, unless a call names an answers_file. The show tool opens the Specify
page on the same answers, with the app and engine built into this command (nothing is
downloaded). Set SPECIFY_APP_DIR to use another app or SPECIFY_WASM_DIR to use a local
wasm-pack pkg/; a build made without the app needs SPECIFY_APP_DIR. See docs/mcp.md.")]
    Mcp {
        /// serve Streamable HTTP on 127.0.0.1 instead of stdio
        #[arg(long)]
        http: bool,
        /// the HTTP port (default: any free port, printed at start)
        #[arg(long, requires = "http")]
        port: Option<u16>,
        /// a directory calls may read and write in (repeatable; default the working directory)
        #[arg(long, value_name = "DIR", action = ArgAction::Append)]
        allow: Vec<PathBuf>,
    },
    /// triangles ranged by face index, for a picker
    Mesh {
        step: PathBuf,
        /// mesh JSON (`-` for stdout; the summary then goes to stderr)
        #[arg(short, long)]
        output: PathBuf,
        /// which part of an assembly (see parts)
        #[arg(long, value_parser = part_number)]
        part: Option<i64>,
        /// maximum surface/trim chord deviation in mm, at least 0.001; refuses unmet bounds. Exact on planes, spheres, line and circle trims; a sampled maximum on cylinders, cones, tori, B-splines and ellipse/B-spline/NURBS trims. Not a watertightness guarantee
        #[arg(long = "max-deviation-mm", allow_hyphen_values = true)]
        max_deviation_mm: Option<f64>,
    },
    /// list the questions, given answers so far
    Questions {
        analysis: PathBuf,
        /// answers JSON: {question id: value}
        #[arg(long)]
        answers: Option<PathBuf>,
        /// only unanswered questions
        #[arg(long)]
        open: bool,
        /// add each default's source: the rule behind it (docs/rule-sources.md)
        #[arg(long)]
        sources: bool,
    },
    /// apply an edit list to a STEP file
    Write {
        step: PathBuf,
        /// edit list (or a 0.2 answers file) applied all or nothing
        #[arg(value_name = "EDITS.json")]
        edits: PathBuf,
        // Recognise the removed command line to give callers a migration message.
        #[arg(hide = true, num_args = 0..)]
        legacy_answers: Vec<String>,
        #[arg(short, long)]
        output: Option<String>,
        /// validate an edit list and print the result without writing
        #[arg(long, conflicts_with = "output")]
        check: bool,
        /// which part of an assembly (see parts)
        #[arg(long, value_parser = part_number)]
        part: Option<i64>,
        #[arg(long, hide = true)]
        accept_defaults: bool,
        #[arg(long, hide = true)]
        bom: bool,
        #[arg(long, hide = true)]
        intent: Option<PathBuf>,
    },
    /// what changed between two revisions of a part or an assembly
    Diff {
        /// the STEP file as it was (a part or an assembly)
        old: PathBuf,
        /// the STEP file as it is now
        new: PathBuf,
        /// a directory: report.json, report.md, model.png
        #[arg(short, long)]
        output: PathBuf,
        /// compare one part of an assembly (see parts)
        #[arg(long, value_parser = part_number)]
        part: Option<i64>,
        /// the report's title (the new file's name)
        #[arg(long)]
        title: Option<String>,
        /// the /diff page's link for these revisions: an assembly's changed parts link to it
        /// with &part=NAME
        #[arg(long)]
        viewer: Option<String>,
        /// include the compared PMI scenes in report.json
        #[arg(long)]
        pmi_scenes: bool,
    },
    /// carry an older revision's requirements onto a new revision of the geometry
    #[command(long_about = "\
Carry OLD's requirements onto NEW, file to file, and write the result to OUT (ADR P11). OLD's
features are matched to NEW's as `diff` matches them. For each feature-and-kind OLD states: a
gap in NEW is filled (carried); one NEW states already keeps NEW's value, and a difference is
reported (kept); one on a feature that moved or changed size is carried and listed to check;
one whose feature is not found, or is matched ambiguously, is not carried and reported.
Decided-none carries the same way, and characteristic numbers (and in an assembly, BOM item
numbers) carry with them. What Specify does not understand is reported not carried, with its
kind and faces. The carry is applied all or nothing: if any of it cannot be applied, nothing
is written (exit 2) and the reasons are reported. The report (JSON, with its Markdown) goes
to stdout. See docs/carry.md.")]
    Carry {
        /// the annotated STEP file of the earlier revision
        old: PathBuf,
        /// the STEP file of the new revision (bare, or with some PMI)
        new: PathBuf,
        /// where to write NEW with OLD's requirements carried (a `.gz` name is gzipped)
        #[arg(short, long)]
        output: PathBuf,
    },
    /// open a STEP file in Specify in your browser, served from this computer
    #[command(long_about = "\
Open a STEP file in Specify in your browser, served by this command on 127.0.0.1: nothing
is uploaded. Without a file, the page opens with a drop target and file picker and Save writes
<name>.specified.step into the current working directory. With a file, Specify's Save writes
the specified file, from the answers given in the page, next to the input as
<name>.specified.step (then -2, -3, ...; later saves update the copy this run wrote, and no
other file is overwritten), or with --in-place over the input (through a link, to its target),
the original kept once as .bak. With --diff OLD NEW the page shows what changed from OLD (as
`specify-pmi diff` reports it) and opens NEW as `open NEW` would; Save writes NEW. Stops 90
seconds after the last page is closed, or after 30 minutes without a request.

The app and its engine are built into this command: nothing is downloaded. Use --app DIR (or
SPECIFY_APP_DIR) for another app and --wasm DIR (or SPECIFY_WASM_DIR) for a local wasm-pack
pkg/ directory (else an app's own wasm/, else the built-in engine). A build made without the
app needs --app.")]
    Open {
        /// a .step or .stp file, or either gzipped (omit to pick a file in the browser)
        #[arg(conflicts_with = "diff")]
        step: Option<PathBuf>,
        /// compare OLD to NEW, and specify NEW
        #[arg(long, num_args = 2, value_names = ["OLD", "NEW"])]
        diff: Option<Vec<PathBuf>>,
        /// refuse answer changes and saves
        #[arg(long, conflicts_with_all = ["in_place", "diff"])]
        read_only: bool,
        /// save over the input, keeping the original once as .bak
        #[arg(long)]
        in_place: bool,
        /// the port on 127.0.0.1 (default: any free port)
        #[arg(long)]
        port: Option<u16>,
        /// print the address to open instead of opening a browser
        #[arg(long)]
        no_browser: bool,
        /// a local Specify app (index.html at its root); overrides SPECIFY_APP_DIR and the built-in app
        #[arg(long, value_name = "DIR")]
        app: Option<PathBuf>,
        /// a local wasm-pack pkg/ directory; overrides SPECIFY_WASM_DIR and the built-in engine
        #[arg(long, value_name = "DIR")]
        wasm: Option<PathBuf>,
    },
    /// print the assembly BOM as JSON
    Bom { step: PathBuf },
    /// print a shell completion script to stdout
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

/// `--part`: an integer, surrounding blanks allowed (as Python's `int`). A negative one is
/// refused when the file is read, as Python's loader refuses it.
fn part_number(text: &str) -> Result<i64, std::num::ParseIntError> {
    text.trim().parse()
}

/// How a run ends: an exit code, and what goes to stderr.
struct Exit {
    code: u8,
    message: String,
}

impl Exit {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Exit {
            code,
            message: message.into(),
        }
    }

    /// An uncaught failure (Python's traceback, exit 1).
    fn error(message: impl std::fmt::Display) -> Self {
        Exit::new(1, format!("error: {message}"))
    }
}

/// The command line's clap command. Every command takes a value that looks like a negative
/// number (`--part -1`, `--title -5`, `parts -1`) as a value, as argparse did, rather than as
/// an unknown option; other values starting with `-` stay unknown options.
fn command() -> clap::Command {
    Cli::command().mut_subcommands(|c| c.allow_negative_numbers(true))
}

/// The arguments parsed, or clap's error (help included, exit 0) to print.
fn parse(argv: &[String]) -> Result<Cmd, clap::Error> {
    let mut matches = command()
        .try_get_matches_from(std::iter::once(PROG).chain(argv.iter().map(String::as_str)))?;
    Ok(Cli::from_arg_matches_mut(&mut matches)?.command)
}

/// Runs the command line on `argv` (the arguments after the program's name) and returns the
/// exit code; replies go to stdout, messages to stderr.
pub fn main(argv: &[String]) -> u8 {
    let command = match parse(argv) {
        Ok(command) => command,
        Err(e) => {
            let _ = e.print();
            return u8::try_from(e.exit_code()).unwrap_or(2);
        }
    };
    let mut stdout = io::stdout().lock();
    let result = run(command, &mut stdout);
    let _ = stdout.flush();
    match result {
        Ok(()) => 0,
        Err(exit) => {
            let _ = writeln!(io::stderr(), "{}", exit.message);
            exit.code
        }
    }
}

fn run(cmd: Cmd, out: &mut impl Write) -> Result<(), Exit> {
    match cmd {
        Cmd::Serve => {
            let stdin = io::stdin().lock();
            serve::serve(stdin, out).map_err(Exit::error)
        }
        Cmd::Mcp { http, port, allow } => mcp(http, port, &allow, out),
        Cmd::Completions { shell } => {
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut command(), PROG, &mut script);
            if shell == clap_complete::Shell::Bash {
                // clap_complete 4.6 names the subcommands' cases with the program name's `-`
                // escaped as a subcommand separator (`specify__subcmd__pmi__subcmd__write`) but
                // selects them as `specify__pmi__subcmd__write`, so no subcommand's options
                // would complete.
                let text = String::from_utf8(script).map_err(Exit::error)?;
                script = text
                    .replace("specify__subcmd__pmi__subcmd__", "specify__pmi__subcmd__")
                    .into_bytes();
            }
            out.write_all(&script).map_err(Exit::error)
        }
        Cmd::Bom { step } => {
            let rows = crate::bom::read_path(&step).map_err(Exit::error)?;
            print(
                out,
                &serde_json::to_string_pretty(&rows).map_err(Exit::error)?,
            )
        }
        Cmd::Parts { step } => {
            let parts = api::parts(&step).map_err(Exit::error)?;
            print(out, &indent1_keeping_non_ascii(&Value::Array(parts)))
        }
        Cmd::Analyse { step, output, part } => {
            let part = part_arg(&step, part)?;
            // The file is read once, for the part and the other parts its mates need.
            let file = part::File::read(&step).map_err(Exit::error)?;
            let loaded = file.load(part).map_err(Exit::error)?;
            let (analysis, warnings) =
                api::analyse_reporting_in(&file, &loaded).map_err(Exit::error)?;
            emit(
                out,
                &output,
                &pyjson::dumps_indent1(&analysis),
                &pyjson::dumps(&summary(&analysis)),
            )?;
            for line in warnings {
                eprintln!("warning: {line}");
            }
            Ok(())
        }
        Cmd::Mesh {
            step,
            output,
            part,
            max_deviation_mm,
        } => {
            let part = part_arg(&step, part)?;
            let mesh = api::mesh_file_with_max_deviation(&step, part, max_deviation_mm)
                .map_err(Exit::error)?;
            let count = |key: &str| mesh[key].as_array().map_or(0, Vec::len);
            let shown = json!({"faces": count("faces"), "triangles": count("indices") / 3});
            emit(
                out,
                &output,
                &pyjson::dumps_compact(&mesh),
                &pyjson::dumps(&shown),
            )?;
            let missing: Vec<String> = mesh["missing"]
                .as_array()
                .into_iter()
                .flatten()
                .map(Value::to_string)
                .collect();
            if !missing.is_empty() {
                eprintln!(
                    "warning: faces [{}] did not triangulate",
                    missing.join(", ")
                );
            }
            Ok(())
        }
        Cmd::Questions {
            analysis,
            answers,
            open,
            sources,
        } => {
            let analysis = read_json(&analysis)?;
            let answers = match answers {
                Some(path) => answers_of(read_json(&path)?)?,
                None => Map::new(),
            };
            let answers = api::with_stored(&analysis, &answers);
            let qs = api::questions(&analysis, &answers).map_err(refused)?;
            let shown: Vec<Value> = if open {
                rules::open_questions(&qs, &answers)
                    .iter()
                    .map(|q| q.to_dict_with(sources))
                    .collect()
            } else {
                qs.iter().map(|q| q.to_dict_with(sources)).collect()
            };
            print(out, &indent1_keeping_non_ascii(&Value::Array(shown)))
        }
        Cmd::Write {
            step,
            edits,
            legacy_answers,
            output,
            check,
            part,
            accept_defaults,
            bom,
            intent,
        } => {
            if !legacy_answers.is_empty() || accept_defaults || bom || intent.is_some() {
                return Err(Exit::new(
                    2,
                    "whole-intent write was removed. Use write STEP EDITS.json -o OUT, \
                     with accept_suggestions in the edit list in place of --accept-defaults. \
                     See https://github.com/pzfreo/specify-pmi/blob/main/docs/answers.md.",
                ));
            }
            let part = part_arg(&step, part)?;
            if check {
                check_edits(&step, &edits, part, out)
            } else {
                let output =
                    output.ok_or_else(|| Exit::new(2, "write requires -o OUT or --check"))?;
                write_edits(&step, &edits, part, &output, out)
            }
        }
        Cmd::Diff {
            old,
            new,
            output,
            part,
            title,
            viewer,
            pmi_scenes,
        } => diff(
            &old,
            &new,
            &Compared {
                output,
                part,
                title,
                viewer,
                scenes: pmi_scenes,
            },
            out,
        ),
        Cmd::Carry { old, new, output } => carry(&old, &new, &output, out),
        Cmd::Open {
            step,
            diff,
            read_only,
            in_place,
            port,
            no_browser,
            app,
            wasm,
        } => {
            let options = crate::open::Options {
                read_only,
                port: port.unwrap_or(0),
                part: None,
            };
            let opening = match (step, diff.as_deref()) {
                (Some(step), _) => Opening::File(step),
                (None, Some([old, new])) => Opening::Diff {
                    old: old.clone(),
                    new: new.clone(),
                },
                (None, None) => Opening::Empty,
                _ => return Err(Exit::new(2, "open [FILE], or open --diff OLD NEW")),
            };

            open(
                &opening,
                in_place,
                &options,
                app.as_deref(),
                wasm.as_deref(),
                no_browser,
                out,
            )
        }
    }
}

/// `write STEP EDITS.json --check`: the edit list (or a 0.2 answers file) parsed and validated
/// against the file's feature table, the outcome printed as JSON; exit 2 with its message when
/// any entry fails. Nothing is written.
fn check_edits(
    step: &Path,
    edits: &Path,
    part: Option<usize>,
    out: &mut impl Write,
) -> Result<(), Exit> {
    let loaded = part::load(step, part).map_err(Exit::error)?;
    let text = std::fs::read_to_string(edits)
        .map_err(|e| Exit::error(format!("{}: {e}", edits.display())))?;
    let outcome = crate::writer::apply::check(&loaded, &text);
    print(out, &indent1_keeping_non_ascii(&outcome.to_json()))?;
    if !outcome.failures.is_empty() {
        return Err(Exit::new(2, format!("cannot write: {}", outcome.message())));
    }
    Ok(())
}

/// `write STEP EDITS.json -o OUT`: the edit list (or a 0.2 answers file) applied to the file
/// in place, all or nothing, and the report printed. A list that cannot be applied writes
/// nothing: exit 2, every failing entry named.
fn write_edits(
    step: &Path,
    edits: &Path,
    part: Option<usize>,
    output: &str,
    out: &mut impl Write,
) -> Result<(), Exit> {
    let loaded = part::load(step, part).map_err(Exit::error)?;
    let text = std::fs::read_to_string(edits)
        .map_err(|e| Exit::error(format!("{}: {e}", edits.display())))?;
    let cannot = |failures: Vec<crate::edits::Failure>| {
        let mut message = "cannot write: nothing was written. An edit list is applied all or \
                           nothing, and these entries cannot be applied:"
            .to_string();
        for f in &failures {
            message.push_str(&format!("\n- {}: {}", f.entry, f.reason));
        }
        Exit::new(2, message)
    };
    let list = crate::edits::read(&text).map_err(cannot)?;
    if let Ok(Value::Object(answers)) = serde_json::from_str(&text)
        && answers.contains_key(crate::answer_file::META)
        && crate::edits::is_answers_file(&answers)
    {
        crate::answer_file::check(&loaded, &answers)
            .map_err(|why| Exit::new(2, format!("answers {}: {why}", edits.display())))?;
        let analysis = api::analyse(&loaded).map_err(Exit::error)?;
        let questions = api::questions(&analysis, &answers).map_err(refused)?;
        crate::answer_file::require_resolved_where(&answers, |id| {
            questions.iter().any(|q| q.id == id)
        })
        .map_err(|e| refused(e.into()))?;
    }
    let mut applied = crate::writer::apply::apply(&loaded, &list).map_err(cannot)?;
    let output_path = Path::new(output);
    let reopened = part::File::of_bytes(output_path, applied.bytes.clone())
        .and_then(|file| file.load(part))
        .map_err(Exit::error)?;
    let analysis = api::analyse(&reopened).map_err(Exit::error)?;
    let missing: Vec<_> = analysis["table"]["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["open"] == json!(true))
        .filter_map(|row| row["id"].as_str())
        .collect();
    crate::writer::put(output_path, std::mem::take(&mut applied.bytes))
        .map_err(|e| Exit::new(1, format!("write failed, {output} left as it was: {e}")))?;
    applied.report.output = output_path.to_path_buf();
    let mut report = applied.to_dict();
    if !missing.is_empty() {
        report["missing"] = json!(missing);
    }
    print(out, &indent1_keeping_non_ascii(&report))
}

/// The rules' refusal as the command line words it: exit 2, `incomplete: ...` or `invalid
/// answer: ...` (a bad datum, a frame no longer offered, JSON that does not parse).
fn refused(e: Error) -> Exit {
    match e {
        Error::Rules(rules::Error::Incomplete(e)) => Exit::new(2, format!("incomplete: {e}")),
        Error::Rules(rules::Error::Invalid(m)) => Exit::new(2, format!("invalid answer: {m}")),
        other => Exit::error(other),
    }
}

/// `--part`, as an index of the file's parts. Python takes any integer and its loader refuses
/// a negative one by the same message as one past the end.
fn part_arg(step: &Path, part: Option<i64>) -> Result<Option<usize>, Exit> {
    let Some(value) = part else {
        return Ok(None);
    };
    match usize::try_from(value) {
        Ok(k) => Ok(Some(k)),
        Err(_) => {
            let count = part::parts(step).map_err(Exit::error)?.len();
            Err(Exit::error(format!(
                "{} has {count} part(s); there is no part {value}",
                step.display()
            )))
        }
    }
}

/// What `open` opens.
enum Opening {
    Empty,
    File(PathBuf),
    Diff { old: PathBuf, new: PathBuf },
}

/// `open`: a session of the file (or of the pair, compared as `diff` compares them, or empty until
/// a file is dropped/picked in the browser), the Specify app served on 127.0.0.1 for it and the
/// browser opened at it (or, with `--no-browser`, only its address printed); then each save's
/// notice and the stop's. Exit 2 for a name Specify does not open, no app, or options that cannot
/// be combined.
fn open(
    opening: &Opening,
    in_place: bool,
    options: &crate::open::Options,
    app: Option<&Path>,
    wasm: Option<&Path>,
    no_browser: bool,
    out: &mut impl Write,
) -> Result<(), Exit> {
    use crate::open::{App, CLOSE_DELAY, Error as OpenError, IDLE, assets, launch_browser};
    use crate::session::{Opened, Session};
    let failed = |e: OpenError| match e {
        OpenError::Refused(m) => Exit::new(2, format!("cannot open: {m}")),
        other => Exit::error(other),
    };
    let named = |path: &Path| {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if crate::open::is_step_name(&name) {
            Ok(())
        } else {
            Err(Exit::new(
                2,
                format!(
                    "cannot open: {} is not a STEP file: Specify opens .step or .stp, or either \
                     gzipped",
                    path.display()
                ),
            ))
        }
    };
    let served = match opening {
        Opening::Empty => {
            if in_place {
                return Err(Exit::new(2, "cannot open: --in-place requires a file"));
            }
            let save_dir = std::env::current_dir().map_err(Exit::error)?;
            let found = assets(app, wasm).map_err(failed)?;
            App::empty(save_dir, found, options).map_err(failed)?
        }
        Opening::File(step) => {
            named(step)?;
            let session = Session::open(Opened::read(step).map_err(Exit::error)?, in_place)
                .map_err(refused)?;
            let found = assets(app, wasm).map_err(failed)?;
            let session = std::sync::Arc::new(std::sync::Mutex::new(session));
            App::new(session, found, options).map_err(failed)?
        }
        Opening::Diff { old, new } => {
            named(old)?;
            named(new)?;
            let old = Opened::read(old).map_err(Exit::error)?;
            let new = Opened::read(new).map_err(Exit::error)?;
            let session = Session::diff(old, new, None, in_place)
                .and_then(|mut session| {
                    session.resume()?;
                    Ok(session)
                })
                .map_err(refused)?;
            let found = assets(app, wasm).map_err(failed)?;
            let session = std::sync::Arc::new(std::sync::Mutex::new(session));
            App::new(session, found, options).map_err(failed)?
        }
    };
    let url = served.url();
    if served.name().is_empty() {
        print(out, &format!("Open: {url}"))?;
    } else {
        print(out, &format!("Open {}: {url}", served.name()))?;
    }
    out.flush().map_err(Exit::error)?;
    if !no_browser && let Err(e) = launch_browser(&url) {
        let _ = writeln!(
            io::stderr(),
            "warning: cannot open a browser ({e}); open the address above"
        );
    }
    let mut written = Ok(());
    served
        .serve(CLOSE_DELAY, IDLE, |notice| {
            if written.is_ok() {
                written = writeln!(out, "{notice}").and_then(|_| out.flush());
            }
        })
        .map_err(failed)?;
    written.map_err(Exit::error)
}

/// `mcp`: the MCP server on stdio, or on HTTP with its address and token printed first.
fn mcp(http: bool, port: Option<u16>, allow: &[PathBuf], out: &mut impl Write) -> Result<(), Exit> {
    use crate::mcp;
    let server = mcp::Server::new(allow).map_err(|e| Exit::new(2, format!("cannot serve: {e}")))?;
    if !http {
        let stdin = io::stdin().lock();
        let mut server = server;
        return mcp::serve_stdio(stdin, out, &mut server).map_err(Exit::error);
    }
    let listener = mcp::bind(port.unwrap_or(0)).map_err(Exit::error)?;
    let bound = listener.local_addr().map_err(Exit::error)?.port();
    let token = mcp::new_token().map_err(Exit::error)?;
    let dirs: Vec<String> = server
        .allowed()
        .iter()
        .map(|d| d.display().to_string())
        .collect();
    print(
        out,
        &format!("MCP endpoint: http://127.0.0.1:{bound}{}", mcp::ENDPOINT),
    )?;
    print(out, &format!("Authorization: Bearer {token}"))?;
    print(out, &format!("Allowed directories: {}", dirs.join(", ")))?;
    out.flush().map_err(Exit::error)?;
    mcp::serve_http(listener, token, server).map_err(Exit::error)
}

/// `diff`'s options.
struct Compared {
    output: PathBuf,
    part: Option<i64>,
    title: Option<String>,
    viewer: Option<String>,
    scenes: bool,
}

/// `diff`: what changed from OLD to NEW, read from the two files' geometry and PMI only, and
/// NEW's picture.
fn diff(old: &Path, new: &Path, args: &Compared, out: &mut impl Write) -> Result<(), Exit> {
    let part = part_arg(new, args.part)?;
    // The part is chosen in both files: say which file lacks it before either is loaded.
    if let Some(k) = part {
        for path in [old, new] {
            let count = part::parts(path).map_err(Exit::error)?.len();
            if k >= count {
                return Err(Exit::error(format!(
                    "{} has {count} part(s); there is no part {k}",
                    path.display()
                )));
            }
        }
    }
    // With no part chosen, a file of several parts, or of a part placed other than once, is
    // compared as an assembly.
    let read = |path: &Path| part::File::read(path).map_err(Exit::error);
    let compared = crate::revision::compare(
        &read(old)?,
        &read(new)?,
        part,
        crate::revision::DiffOptions {
            scenes: args.scenes,
        },
    )
    .map_err(refused)?;
    let dir = &args.output;
    std::fs::create_dir_all(dir).map_err(|e| Exit::error(format!("{}: {e}", dir.display())))?;
    let pretty = |v: &Value| indent1_keeping_non_ascii(v);
    save(&dir.join("report.json"), &pretty(&compared.report()))?;
    let (markdown, png) = match &compared {
        crate::revision::Comparison::Part {
            before,
            after,
            revised,
        } => {
            let title = args.title.clone().unwrap_or_else(|| {
                new.file_name()
                    .map_or("part".into(), |n| n.to_string_lossy().into_owned())
            });
            (
                revised.markdown(&title, Some("model.png")),
                crate::revision::picture(before, after, revised, 900),
            )
        }
        crate::revision::Comparison::Assembly {
            before,
            after,
            compared,
        } => {
            let title = args.title.clone().unwrap_or_else(|| {
                new.file_name()
                    .map_or("assembly".into(), |n| n.to_string_lossy().into_owned())
            });
            (
                compared.markdown(&title, Some("model.png"), args.viewer.as_deref()),
                compared.picture(before, after, 900),
            )
        }
    };
    save(&dir.join("report.md"), &markdown)?;
    let picture = dir.join("model.png");
    std::fs::write(&picture, png)
        .map_err(|e| Exit::error(format!("{}: {e}", picture.display())))?;
    print(
        out,
        &indent1_keeping_non_ascii(
            &json!({"verdict": compared.verdict(), "summary": compared.summary()}),
        ),
    )
}

/// `carry OLD NEW -o OUT`: OLD's requirements carried onto NEW and written to OUT, all or
/// nothing; the report printed. A carry that cannot be applied writes nothing: exit 2, with
/// the report and its reasons.
fn carry(old: &Path, new: &Path, output: &Path, out: &mut impl Write) -> Result<(), Exit> {
    let read = |path: &Path| part::File::read(path).map_err(Exit::error);
    let carried = crate::carry::carry(&read(old)?, &read(new)?).map_err(Exit::error)?;
    let mut report = carried.report;
    if let Some(bytes) = carried.bytes {
        crate::writer::put(output, bytes).map_err(|e| {
            Exit::new(
                1,
                format!("write failed, {} left as it was: {e}", output.display()),
            )
        })?;
    } else {
        report.written = false;
    }
    print(out, &indent1_keeping_non_ascii(&report.to_json()))?;
    // As `diff` flags its own: the counts on stderr, whatever the verdict.
    eprintln!("{}: {}", report.verdict(), report.summary());
    if !report.refused.is_empty() {
        let mut message = "cannot carry: nothing was written. A carry is applied all or \
                           nothing, and these could not be applied:"
            .to_string();
        for f in &report.refused {
            message.push_str(&format!("\n- {}: {}", f.entry, f.reason));
        }
        return Err(Exit::new(2, message));
    }
    Ok(())
}

/// A JSON file, parsed. A file that cannot be read is an uncaught failure (exit 1); one that
/// is not JSON is refused as an invalid answer (exit 2), as Python's `JSONDecodeError` (a
/// `ValueError`) is.
fn read_json(path: &Path) -> Result<Value, Exit> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Exit::error(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| Exit::new(2, format!("invalid answer: {e}")))
}

/// Answers must be a JSON object of question id to value.
fn answers_of(value: Value) -> Result<Map<String, Value>, Exit> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(Exit::new(
            2,
            format!("invalid answer: answers are a JSON object, not {other}"),
        )),
    }
}

/// `analyse`'s stdout: face count, features counted by family in the order families first
/// appear, and how many existing PMI items.
fn summary(analysis: &Value) -> Value {
    let mut counts = Map::new();
    for f in analysis["features"].as_array().into_iter().flatten() {
        let family = f["family"].as_str().unwrap_or_default().to_string();
        let n = counts.get(&family).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(family, json!(n + 1));
    }
    let len = |key: &str| analysis[key].as_array().map_or(0, Vec::len);
    json!({"faces": len("faces"), "features": counts, "existing_pmi": len("existing")})
}

/// Writes `text` to `path` (no newline at the end, as Python's `write_text` of `json.dumps`).
fn save(path: &Path, text: &str) -> Result<(), Exit> {
    std::fs::write(path, text).map_err(|e| Exit::error(format!("{}: {e}", path.display())))
}

/// Writes a command's JSON file and its one-line summary. To a file, the summary goes to stdout;
/// to `-` the JSON goes to stdout and the summary to stderr, so the JSON can be piped into
/// another program (`specify-pmi analyse part.step -o - | jq .features`).
fn emit(out: &mut impl Write, path: &Path, text: &str, shown: &str) -> Result<(), Exit> {
    if path.as_os_str() == "-" {
        print(out, text)?;
        eprintln!("{shown}");
        Ok(())
    } else {
        save(path, text)?;
        print(out, shown)
    }
}

/// A line on stdout.
fn print(out: &mut impl Write, text: &str) -> Result<(), Exit> {
    writeln!(out, "{text}").map_err(Exit::error)
}

/// Python's `json.dumps(value, indent=1, ensure_ascii=False)`: [`pyjson::dumps_indent1`] with
/// every `\uXXXX` escape of a character from U+007F up written as the character itself
/// (surrogate pairs joined). The escapes `ensure_ascii=False` keeps (quotes, backslashes and
/// control characters) are left as they are.
fn indent1_keeping_non_ascii(value: &Value) -> String {
    let text = pyjson::dumps_indent1(value);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let hex = |s: &str| {
            s.get(2..6)
                .filter(|_| s.as_bytes().get(1) == Some(&b'u'))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
        };
        let (decoded, used) = match hex(rest) {
            Some(high @ 0xD800..=0xDBFF) => match hex(&rest[6..]) {
                Some(low @ 0xDC00..=0xDFFF) => (
                    char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)),
                    12,
                ),
                _ => (None, 6),
            },
            Some(code) if code >= 0x7F => (char::from_u32(code), 6),
            Some(_) => (None, 6),
            // `\"`, `\\`, `\n` and the like: the backslash and the character after it.
            None => (None, 2),
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..used]),
        }
        rest = &rest[used..];
    }
    out.push_str(rest);
    out
}
