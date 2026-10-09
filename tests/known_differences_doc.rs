//! `docs/known-differences.md` is generated from `tests/fixtures/known_differences/*.json`, and
//! this test keeps it current: it renders every area file (each entry checked by
//! `common::load_known`) and asserts the committed document equals the rendering.
//!
//! After an area file changes, regenerate the document with
//!
//! ```text
//! UPDATE_KNOWN_DIFFERENCES=1 cargo test --release --test known_differences_doc
//! ```
//!
//! which writes the rendering over `docs/known-differences.md` (and passes); commit it with the
//! area file. The rendering is deterministic: areas in file-name order, verdicts in
//! `common::VERDICTS` order, patterns in the order they first appear in their area file.
//!
//! The document has three parts: a tally of entries per area and verdict; per area and verdict,
//! each pattern with its entries and evidence; and the open work, every `rust-wrong` and
//! `undetermined` entry listed by its case and path. It counts entries, not differences: an
//! entry is a pair of globs that may explain many differences, and each area's own sweep prints
//! the tally of the differences it found.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{KnownDifference, VERDICTS, fixtures, load_known};

/// The environment variable that makes the test write the rendering instead of comparing.
const UPDATE: &str = "UPDATE_KNOWN_DIFFERENCES";

/// Verdicts that are open work: a bug kept visible, or a question nobody has settled.
const OPEN: [&str; 2] = ["rust-wrong", "undetermined"];

fn document() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/known-differences.md")
}

/// Every area file, `(area, entries)`, in file-name order.
fn areas() -> Vec<(String, Vec<KnownDifference>)> {
    let dir = fixtures().join("known_differences");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let area = path.file_stem().unwrap().to_string_lossy().into_owned();
            let entries = load_known(&path);
            (area, entries)
        })
        .collect()
}

/// `text` safe as Markdown prose: `*`, `<`, `|` and `\\` escaped outside
/// backtick code spans, so a glob such as `missing[*]` in evidence is not read as emphasis or
/// a tag. Code spans are kept as written.
fn prose(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for c in text.chars() {
        if c == '`' {
            in_code = !in_code;
        } else if !in_code && matches!(c, '*' | '<' | '|' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `text` as a code span (case names and paths never hold a backtick; checked).
fn code(text: &str) -> String {
    assert!(!text.contains('`'), "a backtick in {text:?}");
    format!("`{text}`")
}

/// The patterns of `entries` with verdict `verdict`, in first-appearance order, each with its
/// entries.
fn patterns<'a>(
    entries: &'a [KnownDifference],
    verdict: &str,
) -> Vec<(&'a str, Vec<&'a KnownDifference>)> {
    let mut groups: Vec<(&str, Vec<&KnownDifference>)> = Vec::new();
    for entry in entries.iter().filter(|e| e.verdict == verdict) {
        match groups.iter_mut().find(|(p, _)| *p == entry.pattern) {
            Some((_, members)) => members.push(entry),
            None => groups.push((&entry.pattern, vec![entry])),
        }
    }
    groups
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// One pattern of the per-area section: where it applies and the evidence for its verdict.
fn render_pattern(out: &mut String, area: &str, pattern: &str, members: &[&KnownDifference]) {
    out.push_str(&format!("- **{}**", prose(pattern)));
    if let [only] = members {
        out.push_str(&format!(
            " ({} at {})\n",
            code(&only.case),
            code(&only.path)
        ));
    } else {
        let mut cases: Vec<&str> = members.iter().map(|e| e.case.as_str()).collect();
        cases.sort_unstable();
        cases.dedup();
        out.push_str(&format!(
            " ({}, {})\n",
            plural(members.len(), "entry", "entries"),
            plural(cases.len(), "case glob", "case globs")
        ));
    }
    let first = &members[0].evidence;
    if members.iter().all(|e| &e.evidence == first) {
        out.push_str(&format!("\n  {}\n\n", prose(first)));
    } else {
        let mut distinct: Vec<&str> = members.iter().map(|e| e.evidence.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        out.push_str(&format!(
            "\n  The evidence is stated per entry ({} distinct; all in `{area}.json`). The first, \
             for {} at {}: {}\n\n",
            distinct.len(),
            code(&members[0].case),
            code(&members[0].path),
            prose(first)
        ));
    }
}

/// The whole document.
fn render(areas: &[(String, Vec<KnownDifference>)]) -> String {
    let mut out = String::new();
    out.push_str(
        "# Known differences from specify-core (Python)\n\n\
         <!-- Generated by tests/known_differences_doc.rs from \
         tests/fixtures/known_differences/*.json. Do not edit: change the area file, then run\n     \
         UPDATE_KNOWN_DIFFERENCES=1 cargo test --release --test known_differences_doc -->\n\n\
         This document is generated from the area files in \
         [`tests/fixtures/known_differences/`](../tests/fixtures/known_differences/) (their \
         [README](../tests/fixtures/known_differences/README.md) gives the format and the rule) \
         and checked to be current by `tests/known_differences_doc.rs`. Regenerate it with \
         `UPDATE_KNOWN_DIFFERENCES=1 cargo test --release --test known_differences_doc`.\n\n\
         Python's outputs are the oracle, not the specification. Each difference a comparison \
         finds must match an entry, a pair of globs over the case and the path, with a verdict \
         (`rust-correct`, `rust-wrong`, `equivalent`, `undetermined`, `not-applicable`) and the \
         evidence for it; a difference no entry matches, or an entry that matches nothing, fails \
         the area's sweep. The tallies below count **entries**: one entry may explain many \
         differences, and each area's sweep prints the tally of the differences it found.\n\n",
    );

    // Summary table.
    out.push_str("## Entries by area and verdict\n\n| Area |");
    for verdict in VERDICTS {
        out.push_str(&format!(" {verdict} |"));
    }
    out.push_str(" Total |\n|---|");
    for _ in 0..=VERDICTS.len() {
        out.push_str("---:|");
    }
    out.push('\n');
    let mut totals = [0usize; VERDICTS.len()];
    for (area, entries) in areas {
        out.push_str(&format!("| [{area}](#{area}) |"));
        for (i, verdict) in VERDICTS.iter().enumerate() {
            let n = entries.iter().filter(|e| e.verdict == *verdict).count();
            totals[i] += n;
            out.push_str(&format!(" {n} |"));
        }
        out.push_str(&format!(" {} |\n", entries.len()));
    }
    out.push_str("| **all** |");
    for n in totals {
        out.push_str(&format!(" {n} |"));
    }
    out.push_str(&format!(" {} |\n\n", totals.iter().sum::<usize>()));

    // Per area, per verdict, the patterns with their evidence.
    for (area, entries) in areas {
        out.push_str(&format!("## {area}\n\n"));
        if entries.is_empty() {
            out.push_str(&format!(
                "No entries: every comparison in this area equals Python's (`{area}.json` is \
                 empty).\n\n"
            ));
            continue;
        }
        for verdict in VERDICTS {
            let groups = patterns(entries, verdict);
            if groups.is_empty() {
                continue;
            }
            let n: usize = groups.iter().map(|(_, m)| m.len()).sum();
            out.push_str(&format!(
                "### {area}: {verdict} ({})\n\n",
                plural(n, "entry", "entries")
            ));
            for (pattern, members) in &groups {
                render_pattern(&mut out, area, pattern, members);
            }
        }
    }

    // Open work: every rust-wrong and undetermined entry, by case and path.
    out.push_str(
        "## Open work\n\n\
         Every `rust-wrong` entry (a bug kept visible) and every `undetermined` entry (a \
         difference whose verdict is not settled), grouped by area, verdict and pattern, each \
         entry as its case glob and the path globs that case's entries name. The evidence for \
         each pattern is in its area's section above.\n\n",
    );
    let mut any = false;
    for (area, entries) in areas {
        for verdict in OPEN {
            let groups = patterns(entries, verdict);
            if groups.is_empty() {
                continue;
            }
            any = true;
            out.push_str(&format!("### {area}: {verdict}\n\n"));
            for (pattern, members) in &groups {
                out.push_str(&format!(
                    "- **{}** ({})\n",
                    prose(pattern),
                    plural(members.len(), "entry", "entries")
                ));
                // Each case once, its paths in file order (a case may hold one path twice
                // under different entries only if the file repeats it; kept as written).
                let mut by_case: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
                for entry in members {
                    by_case.entry(&entry.case).or_default().push(&entry.path);
                }
                for (case, paths) in by_case {
                    let paths: Vec<String> = paths.iter().map(|p| code(p)).collect();
                    out.push_str(&format!("  - {}: {}\n", code(case), paths.join(", ")));
                }
            }
            out.push('\n');
        }
    }
    if !any {
        out.push_str("None.\n\n");
    }
    // One trailing newline.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

#[test]
fn known_differences_md_is_the_rendering_of_the_area_files() {
    let rendered = render(&areas());
    let path = document();
    if std::env::var_os(UPDATE).is_some() {
        std::fs::write(&path, &rendered).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    if committed != rendered {
        let line = committed
            .lines()
            .zip(rendered.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| committed.lines().count().min(rendered.lines().count()));
        panic!(
            "{} is not the rendering of tests/fixtures/known_differences/*.json (first \
             difference at line {}). Regenerate it with\n  {UPDATE}=1 cargo test --release \
             --test known_differences_doc",
            path.display(),
            line + 1
        );
    }
}

/// The open-work section lists every rust-wrong and undetermined entry exactly once, and
/// nothing else: counted from the rendering, against the area files.
#[test]
fn open_work_lists_every_rust_wrong_and_undetermined_entry() {
    let areas = areas();
    let rendered = render(&areas);
    let open = &rendered[rendered.find("## Open work").expect("open work section")..];
    let listed: usize = open
        .lines()
        .filter_map(|l| l.strip_prefix("  - "))
        .map(|l| l.matches("`, `").count() + 1)
        .sum();
    let expected = areas
        .iter()
        .flat_map(|(_, entries)| entries)
        .filter(|e| OPEN.contains(&e.verdict.as_str()))
        .count();
    assert_eq!(listed, expected);
}
