//! Gzipped STEP input (maintainer decision 2026-10-15): gzip is told by its magic bytes and
//! decompressed in memory, so a `.step` and its `.step.gz` read, bind and write identically; a
//! destination named `.gz` is written gzipped.

use std::path::{Path, PathBuf};
use std::process::Command;

use specify_core::{part, resume};

fn fixture(stem: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(format!("{stem}.step"))
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut encoder, bytes).unwrap();
    encoder.finish().unwrap()
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut text = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(bytes), &mut text).unwrap();
    text
}

/// A fresh scratch directory holding `<stem>.step` and its gzip, `<stem>.step.gz`.
fn scratch(name: &str, stem: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("gzip-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let plain = dir.join(format!("{stem}.step"));
    let gz = dir.join(format!("{stem}.step.gz"));
    let text = std::fs::read(fixture(stem)).unwrap();
    std::fs::write(&plain, &text).unwrap();
    std::fs::write(&gz, gzip(&text)).unwrap();
    (dir, plain, gz)
}

/// Runs the binary with `args`: exit code, stdout, stderr.
fn run(args: &[&Path]) -> (i32, Vec<u8>, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        out.stdout,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn p(s: &str) -> &Path {
    Path::new(s)
}

/// Each command that reads a STEP file gives byte-identical output for the gzipped file.
#[test]
fn every_command_reads_a_gzipped_file_as_the_plain_one() {
    for stem in ["plate", "assembly"] {
        let (dir, plain, gz) = scratch(&format!("cli-{stem}"), stem);
        let ran = run(&[p("parts"), &plain]);
        assert_eq!(ran.0, 0, "{}", ran.2);
        assert_eq!(run(&[p("parts"), &gz]), ran, "{stem} parts");
        let part_args: &[&Path] = if stem == "assembly" {
            &[p("--part"), p("1")]
        } else {
            &[]
        };
        for command in ["analyse", "mesh"] {
            let mut outputs = Vec::new();
            for (input, out) in [(&plain, "plain.json"), (&gz, "gz.json")] {
                let out = dir.join(format!("{command}-{out}"));
                let mut args = vec![p(command), input.as_path(), p("-o"), out.as_path()];
                args.extend_from_slice(part_args);
                let ran = run(&args);
                assert_eq!(ran.0, 0, "{stem} {command}: {}", ran.2);
                outputs.push((ran, std::fs::read(&out).unwrap()));
            }
            assert_eq!(outputs[0], outputs[1], "{stem} {command}");
        }
        // write, from the analysis of the plain file, of each input to a plain destination.
        let analysis = dir.join("analyse-plain.json");
        let answers = dir.join("answers.json");
        std::fs::write(&answers, r#"{"part.material": "steel"}"#).unwrap();
        let mut written = Vec::new();
        for (input, out) in [(&plain, "from-plain.step"), (&gz, "from-gz.step")] {
            let out = dir.join(out);
            let ran = run(&[
                p("write"),
                input,
                &analysis,
                &answers,
                p("-o"),
                &out,
                p("--accept-defaults"),
            ]);
            assert_eq!(ran.0, 0, "{stem} write: {}", ran.2);
            written.push(std::fs::read(&out).unwrap());
        }
        assert_eq!(written[0], written[1], "{stem} write");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The binding (the decompressed bytes' sha256) and the stored-answers fingerprint are the same
/// for a file and its gzip, read from a path or from bytes.
#[test]
fn a_gzipped_file_binds_and_fingerprints_as_the_plain_one() {
    let (dir, plain, gz) = scratch("binding", "plate");
    let a = part::load(&plain, None).unwrap();
    let b = part::load(&gz, None).unwrap();
    let c = part::load_bytes("plate.step.gz", std::fs::read(&gz).unwrap()).unwrap();
    assert_eq!(a.binding, b.binding);
    assert_eq!(a.binding, c.binding);
    assert_eq!(
        part::File::read(&gz).unwrap().sha256(),
        part::File::read(&plain).unwrap().sha256()
    );
    let fingerprint = resume::fingerprint(&a).unwrap();
    assert_eq!(resume::fingerprint(&b).unwrap(), fingerprint);
    assert_eq!(resume::fingerprint(&c).unwrap(), fingerprint);
    assert_eq!(part::parts(&gz).unwrap(), part::parts(&plain).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A destination named `.gz` is written gzipped (the STEP text the plain destination gets) and
/// reads back; a write in place to a `.gz` input keeps it gzipped.
#[test]
fn a_gz_destination_is_written_gzipped_and_reads_back() {
    let (dir, plain, gz) = scratch("write", "plate");
    let analysis = dir.join("analysis.json");
    let ran = run(&[p("analyse"), &plain, p("-o"), &analysis]);
    assert_eq!(ran.0, 0, "{}", ran.2);
    let answers = dir.join("answers.json");
    std::fs::write(&answers, r#"{"part.material": "steel"}"#).unwrap();
    let write = |input: &Path, out: &Path| {
        let ran = run(&[
            p("write"),
            input,
            &analysis,
            &answers,
            p("-o"),
            out,
            p("--accept-defaults"),
        ]);
        assert_eq!(ran.0, 0, "{}", ran.2);
    };
    let out_plain = dir.join("out.step");
    let out_gz = dir.join("out.step.gz");
    write(&plain, &out_plain);
    write(&plain, &out_gz);
    let text = std::fs::read(&out_plain).unwrap();
    assert!(text.starts_with(b"ISO-10303-21;"));
    let zipped = std::fs::read(&out_gz).unwrap();
    assert!(zipped.starts_with(&[0x1f, 0x8b]), "not gzipped");
    assert_eq!(gunzip(&zipped), text);
    let read = part::load(&out_gz, None).unwrap();
    assert_eq!(read.binding, part::load(&out_plain, None).unwrap().binding);

    // In place: the gzipped input stays gzipped, and holds what the plain write holds.
    write(&gz, &gz);
    let zipped = std::fs::read(&gz).unwrap();
    assert!(zipped.starts_with(&[0x1f, 0x8b]), "in place: not gzipped");
    assert_eq!(gunzip(&zipped), text);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file that begins as gzip but does not decompress is refused, saying so.
#[test]
fn a_corrupt_gzip_is_refused_with_a_clear_error() {
    let (dir, _, gz) = scratch("corrupt", "plate");
    let mut bytes = std::fs::read(&gz).unwrap();
    bytes.truncate(bytes.len() / 2);
    std::fs::write(&gz, &bytes).unwrap();
    let err = part::load(&gz, None).unwrap_err().to_string();
    assert!(
        err.starts_with(&format!("cannot read STEP file {}", gz.display()))
            && err.contains("gzip but cannot be decompressed"),
        "{err}"
    );
    let err = part::load_bytes("x.step", vec![0x1f, 0x8b, 0, 1, 2])
        .unwrap_err()
        .to_string();
    assert!(err.contains("gzip but cannot be decompressed"), "{err}");
    let (code, _, stderr) = run(&[p("parts"), &gz]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("gzip but cannot be decompressed"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// serve's file loads (parts, analyse) answer a gzipped file as the plain one.
#[test]
fn serve_reads_a_gzipped_file_as_the_plain_one() {
    let (dir, plain, gz) = scratch("serve", "plate");
    let mut requests = String::new();
    for (id, step) in [(1, &plain), (2, &gz)] {
        for op in ["parts", "analyse"] {
            requests += &format!(
                "{}\n",
                serde_json::json!({"id": id, "op": op, "step": step})
            );
        }
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("serve")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), requests.as_bytes()).unwrap();
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    let replies: Vec<serde_json::Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 4, "{replies:?}");
    for (a, b) in [(0, 2), (1, 3)] {
        assert_eq!(replies[a]["ok"], true, "{}", replies[a]);
        assert_eq!(replies[a]["result"], replies[b]["result"]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
