//! `specify-core`: the command line, [`specify_core::cli`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(specify_core::cli::main(&argv))
}
