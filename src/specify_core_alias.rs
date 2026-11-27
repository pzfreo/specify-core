//! `specify-core`: the old name of `specify-pmi`, kept for all of 0.3.x and removed in 0.4.

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(specify_pmi::cli::alias_main(&argv))
}
