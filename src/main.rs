//! Purpose: thin binary entry point for bajan.
//! Responsibilities: parse CLI args, print the emitted envelope (JSON mode)
//! or its text rendering (human mode).
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
}