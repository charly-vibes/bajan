//! Purpose: pdf2bajan — convert a PDF file to a bajan episode-stream JSON
//! array on stdout (bajan-74r upstream converter). PDF is binary, so the
//! input is a file argument, not stdin.
//! Responsibilities: read the file, delegate to `bajan_converters::pdf_episodes`,
//! print the stream. Pipe straight into ingest:
//! `pdf2bajan doc.pdf | bajan --db graph.db ingest`.
//! Rationale: format parsing stays outside bajan (ic_no_format_parsing);
//! this binary is the boundary where a PDF becomes the published
//! episode-stream schema. Every episode carries a `pdf-extractor:` tag —
//! PDF extraction is lossy by design and extractor-version changes change
//! output (see README). Corruption fails honestly with a nonzero exit —
//! never a partial stream.

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: pdf2bajan <file.pdf>");
            std::process::exit(2);
        }
    };
    match std::fs::read(&path) {
        Ok(bytes) => match bajan_converters::pdf_episodes(&bytes) {
            Ok(episodes) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&episodes).expect("episode records serialize")
                );
            }
            Err(err) => {
                eprintln!("pdf2bajan: conversion failed: {err}");
                std::process::exit(1);
            }
        },
        Err(err) => {
            eprintln!("pdf2bajan: cannot read {path}: {err}");
            std::process::exit(1);
        }
    }
}
