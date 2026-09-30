//! Purpose: epub2bajan — convert an EPUB file to a bajan episode-stream
//! JSON array on stdout (bajan-buy upstream converter). EPUB is a binary
//! ZIP container, so the input is a file argument, not stdin.
//! Responsibilities: read the file, delegate to
//! `bajan_converters::epub_episodes`, print the stream. Pipe straight into
//! ingest: `epub2bajan book.epub | bajan --db graph.db ingest`.
//! Rationale: format parsing stays outside bajan (ic_no_format_parsing);
//! this binary is the boundary where an EPUB becomes the published
//! episode-stream schema. Container failures (corruption, DRM, non-EPUB
//! zip) fail honestly with a nonzero exit — never a partial stream.

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: epub2bajan <file.epub>");
            std::process::exit(2);
        }
    };
    match std::fs::read(&path) {
        Ok(bytes) => match bajan_converters::epub_episodes(&bytes) {
            Ok(episodes) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&episodes).expect("episode records serialize")
                );
            }
            Err(err) => {
                eprintln!("epub2bajan: conversion failed: {err}");
                std::process::exit(1);
            }
        },
        Err(err) => {
            eprintln!("epub2bajan: cannot read {path}: {err}");
            std::process::exit(1);
        }
    }
}
