//! Purpose: html2bajan — convert an HTML document on stdin to a bajan
//! episode-stream JSON array on stdout (bajan-quq upstream converter).
//! Responsibilities: read stdin, delegate to `bajan_converters::html_episodes`,
//! print the stream. Pipe straight into ingest:
//! `html2bajan < page.html | bajan --db graph.db ingest`.
//! Rationale: format parsing stays outside bajan (ic_no_format_parsing);
//! this binary is the boundary where HTML becomes the published
//! episode-stream schema.

fn main() {
    use std::io::Read;
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    match bajan_converters::html_episodes(&input) {
        Ok(episodes) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&episodes).expect("episode records serialize")
            );
        }
        Err(err) => {
            eprintln!("html2bajan: conversion failed: {err}");
            std::process::exit(1);
        }
    }
}
