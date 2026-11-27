//! `specify-pmi`: the command line, [`specify_pmi::cli`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // A copy of this binary named `specify-core` (the release archives ship one) is the alias.
    let alias = std::env::args_os()
        .next()
        .is_some_and(|a| std::path::Path::new(&a).file_stem() == Some("specify-core".as_ref()));
    ExitCode::from(if alias {
        specify_pmi::cli::alias_main(&argv)
    } else {
        specify_pmi::cli::main(&argv)
    })
}
