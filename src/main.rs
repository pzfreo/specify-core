//! `specify-core`: the command line (port of `cli.py`): `parts`, `analyse`, `mesh`, `questions`,
//! `write` and `serve`, with specify-core's arguments and JSON formats. Not yet ported: see
//! docs/design.md, stage 4. Until then every command is refused rather than half done.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!(
        "specify-core: not implemented yet (the Rust port is in progress; see docs/design.md)"
    );
    ExitCode::FAILURE
}
