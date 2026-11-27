//! The `specify-core` command: prints a rename notice on stderr, then runs `specify-pmi`.

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(i32::from(specify_pmi::cli::alias_main(&argv)));
}
