//! Purpose: thin binary entry point for bajan.
//! Responsibilities: parse CLI args, print the emitted envelope (JSON mode)
//! or its text rendering (human mode), and exit non-zero when the envelope
//! reports failure.
//! Rationale: all dispatch and envelope construction lives in `cli` so the
//! binary layer stays thin and the logic stays unit-tested.

use bajan::cli::{self, Cli};
use clap::Parser;

fn main() {
    let cli = Cli::parse();
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