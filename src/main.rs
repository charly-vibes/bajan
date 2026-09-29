//! Purpose: thin binary entry point for bajan.
//! Responsibilities: parse CLI args, print the emitted envelope (JSON mode)
//! or its text rendering (human mode), and exit non-zero when the envelope
//! reports failure.
//! Rationale: all dispatch and envelope construction lives in `cli` so the
//! binary layer stays thin and the logic stays unit-tested.

use bajan::cli::{self, Cli};
use clap::Parser;

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // Help/version output is not a failure: print clap's text and
            // exit 0 — only real argument errors get the envelope treatment.
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                err.print().expect("help/version output prints");
                std::process::exit(0);
            }
            // bajan-ts6: a bad invocation must still emit the suite envelope,
            // never clap's plain-text usage — JSON on stdout, exit non-zero.
            let json = cli::argument_error_envelope(&err);
            println!("{json}");
            std::process::exit(cli::exit_code(&json));
        }
    };
    let json = cli::run(cli.command);
    if cli.json {
        println!("{json}");
    } else {
        println!("{}", cli::render_text(&json));
    }
    // Exit status mirrors the envelope (bajan-aan): ok:false is never
    // reported as process success to machine consumers.
    std::process::exit(cli::exit_code(&json));
}
