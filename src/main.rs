//! `specify-pmi`: the command line, [`specify_pmi::cli`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(specify_pmi::cli::main(&argv))
}
