//! Purpose: red-first schema-conformance tests for the upstream converters
//! (bajan-quq) — converter output must be a valid bajan episode stream.
//! Responsibilities: every emitted record parses as
//! `bajan::ingest::EpisodeRecord` (ic_stream-schema) and persists through
//! the real ingest path (`bajan::ingest::persist_stream`) with all-persisted
//! outcomes (ic_verbatim), round-trips idempotently (ic_idempotent), and the
//! converter mapping rules hold (headings → locators, absent marker when
//! none derivable, deterministic ids). The path dep makes structural
//! conformance a compile-time fact; these tests prove the emitted JSON and
//! the ingest interaction behave.
//! Rationale: the ticket's meter — "episode-stream schema round-trip via
//! bajan ingest" — is the acceptance gate, exercised here against the
//! actual store, not a copy of the schema.

use bajan::ingest::{EpisodeRecord, IngestOutcome, Locator};
use bajan::store::sqlite::SqliteStore;
use bajan_converters::{html_episodes, markdown_episodes};

fn persist_all(records: &[EpisodeRecord]) -> Vec<IngestOutcome> {
    let db = SqliteStore::open(":memory:").expect("in-memory store");
    persist_stream_on(&db, records)
}

fn persist_stream_on(db: &SqliteStore, records: &[EpisodeRecord]) -> Vec<IngestOutcome> {
    bajan::ingest::persist_stream(db, records)
        .into_iter()
        .map(|r| r.expect("no store errors on a fresh store"))
        .collect()
}

fn outcome_of(o: &IngestOutcome) -> &'static str {
    match o {
        IngestOutcome::Persisted => "persisted",
        IngestOutcome::AlreadyPersisted => "already_persisted",
        IngestOutcome::Rejected { .. } => "rejected",
    }
}

// --- markdown mapping ---------------------------------------------------

/// Headings become `heading:<text>` locators; text under them carries the
/// anchor (ic_verbatim: structure the converter derives, bajan persists).
#[test]
fn md_heading_becomes_locator() {
    let eps = markdown_episodes("# Roadmap\n\nShip the thing.\n").expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].locator, Locator::Span("heading:Roadmap".into()));
    assert_eq!(eps[0].text, "Ship the thing.");
}

/// Consecutive paragraphs under one heading join into one episode.
#[test]
fn md_paragraphs_under_heading_join() {
    let eps = markdown_episodes("# A\n\nFirst.\n\nSecond.\n").expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].text, "First.\n\nSecond.");
}

/// A document with no headings at all: one episode with the typed absent
/// marker (ic_verbatim: explicit absent, never a fake span) — and it must
/// still persist through bajan.
#[test]
fn md_no_headings_yields_absent_locator_and_persists() {
    let eps = markdown_episodes("Just some prose, no structure.\n").expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].locator, Locator::Absent);
    let outcomes = persist_all(&eps);
    assert_eq!(outcomes, vec![IngestOutcome::Persisted]);
}

/// Nested headings: each heading opens a new episode keyed on its own
/// anchor.
#[test]
fn md_nested_headings_split_episodes() {
    let eps = markdown_episodes("# Top\n\nIntro.\n\n## Sub\n\nDetail.\n\n# Next\n\nMore.\n")
        .expect("converts");
    assert_eq!(eps.len(), 3);
    assert_eq!(eps[0].locator, Locator::Span("heading:Top".into()));
    assert_eq!(eps[1].locator, Locator::Span("heading:Sub".into()));
    assert_eq!(eps[2].locator, Locator::Span("heading:Next".into()));
    assert_eq!(eps[1].text, "Detail.");
}

// --- id determinism ------------------------------------------------------

/// Ids are deterministic for the same input (stable ids are the identity
/// bajan's resubmission policy hangs off).
#[test]
fn md_ids_deterministic() {
    let input = "# H\n\nOne.\n\n## H\n\nTwo.\n";
    let a = markdown_episodes(input).expect("converts");
    let b = markdown_episodes(input).expect("converts");
    assert_eq!(a, b);
    assert_ne!(
        a[0].id, a[1].id,
        "same-heading anchors still get distinct ids"
    );
}

/// Ids are unique within one converted stream even when anchors differ
/// only by case: the slug lowercases, so disambiguation must count per
/// DERIVED slug (bajan-a4d) — 'Notes' and 'notes' cannot both be `notes`.
#[test]
fn md_ids_unique_across_case_variant_anchors() {
    let eps = markdown_episodes("# Notes\n\nOne.\n\n# notes\n\nTwo.\n").expect("converts");
    assert_eq!(eps.len(), 2);
    assert_ne!(eps[0].id, eps[1].id, "case-variant anchors collide on slug");
}

/// Ids are unique even when one heading's slug equals another anchor's
/// disambiguator form: 'Notes', 'Notes', 'Notes 2' — the third heading
/// must not receive the same `notes-2` the disambiguator produced.
#[test]
fn md_ids_unique_when_heading_text_mimics_disambiguator() {
    let eps = markdown_episodes("# Notes\n\nOne.\n\n# Notes\n\nTwo.\n\n# Notes 2\n\nThree.\n")
        .expect("converts");
    assert_eq!(eps.len(), 3);
    let mut ids: Vec<&str> = eps.iter().map(|e| e.id.as_str()).collect();
    ids.sort();
    assert_eq!(
        ids.windows(2).filter(|w| w[0] == w[1]).count(),
        0,
        "duplicate ids in one stream: {ids:?}"
    );
}

/// Stream-wide id uniqueness for html too, over case variants and
/// disambiguator look-alikes in one document.
#[test]
fn html_ids_unique_across_case_variant_anchors() {
    let eps = html_episodes(
        "<h1>Notes</h1><p>One.</p><h1>notes</h1><p>Two.</p><h1>notes-2</h1><p>Three.</p>",
    )
    .expect("converts");
    assert_eq!(eps.len(), 3);
    let mut ids: Vec<&str> = eps.iter().map(|e| e.id.as_str()).collect();
    ids.sort();
    assert_eq!(
        ids.windows(2).filter(|w| w[0] == w[1]).count(),
        0,
        "duplicate ids in one stream: {ids:?}"
    );
}

/// Same document converted twice and ingested into the same store:
/// second submission is already_persisted, never a duplicate (ic_idempotent).
#[test]
fn md_round_trip_idempotent() {
    let eps = markdown_episodes("# H\n\nBody.\n").expect("converts");
    let db = SqliteStore::open(":memory:").expect("store");
    assert_eq!(persist_stream_on(&db, &eps), vec![IngestOutcome::Persisted]);
    assert_eq!(
        persist_stream_on(&db, &markdown_episodes("# H\n\nBody.\n").expect("converts")),
        vec![IngestOutcome::AlreadyPersisted]
    );
}

// --- control bytes -------------------------------------------------------

/// C0 control bytes from pdf-extract (e.g. \x01 bullet artifacts) must never
/// survive into episode text (bajan-6jp) — they would corrupt ic_verbatim
/// prose and downstream claims. collapse() strips all C0 controls except
/// \n and \t (which split_whitespace collapses anyway). Exercised through
/// the markdown path, which shares the same collapse() as the pdf path.
#[test]
fn md_control_bytes_stripped_from_text() {
    let eps =
        markdown_episodes("# Audits\n\n\u{1} In 1989, the auditor resigned.\n").expect("converts");
    assert_eq!(eps.len(), 1);
    let text = &eps[0].text;
    assert_eq!(text, "In 1989, the auditor resigned.");
    assert!(
        !text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "control bytes leaked into episode text: {text:?}"
    );
}

// --- html mapping --------------------------------------------------------

/// h1..h6 elements become heading anchors; text between them joins into
/// episodes under the nearest preceding heading.
#[test]
fn html_headings_become_locators() {
    let eps = html_episodes("<h1>Spec</h1><p>Normative text.</p><h2>Detail</h2><p>More text.</p>")
        .expect("converts");
    assert_eq!(eps.len(), 2);
    assert_eq!(eps[0].locator, Locator::Span("heading:Spec".into()));
    assert_eq!(eps[0].text, "Normative text.");
    assert_eq!(eps[1].locator, Locator::Span("heading:Detail".into()));
    assert_eq!(eps[1].text, "More text.");
}

/// Leading content before any heading gets the absent locator and still
/// persists (locator-absent-marker contract).
#[test]
fn html_leading_content_absent_locator() {
    let eps = html_episodes("<p>Preamble.</p><h1>H</h1><p>Body.</p>").expect("converts");
    assert_eq!(eps.len(), 2);
    assert_eq!(eps[0].locator, Locator::Absent);
    assert_eq!(eps[0].text, "Preamble.");
    assert_eq!(eps[1].locator, Locator::Span("heading:H".into()));
    let outcomes = persist_all(&eps);
    assert!(outcomes.iter().all(|o| outcome_of(o) == "persisted"));
}

/// Script/style content never leaks into episode text.
#[test]
fn html_script_style_dropped() {
    let eps = html_episodes("<h1>T</h1><style>.x{}</style><script>evil()</script><p>Real.</p>")
        .expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].text, "Real.");
}

/// Heading anchors exclude script/style descendants — anchor text is
/// source structure, never embedded code (bajan-tx4); the slug id follows
/// the clean anchor.
#[test]
fn html_heading_anchor_excludes_script_style() {
    let eps =
        html_episodes("<h1>Title<script>alert(1)</script><style>.x{}</style></h1><p>Body.</p>")
            .expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].locator, Locator::Span("heading:Title".into()));
    assert_eq!(eps[0].id, "title");
    assert_eq!(eps[0].text, "Body.");
}

/// Head metadata (<title>, <style> inside <head>) is document metadata,
/// not prose — it never becomes episode content (bajan-tx4 family).
#[test]
fn html_title_head_never_leak() {
    let eps = html_episodes(
        "<html><head><title>Page Title</title><style>s{}</style></head><body><h1>H</h1><p>Body.</p></body></html>",
    )
    .expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].text, "Body.");
    assert_eq!(eps[0].locator, Locator::Span("heading:H".into()));
}

/// Block-level siblings are separate paragraphs: list items, blockquote
/// content, and ordered-list items each end with a paragraph boundary —
/// never glued without a word boundary (bajan-dp2, ic_verbatim).
#[test]
fn html_list_items_and_blockquotes_are_separate_paragraphs() {
    let eps = html_episodes(
        "<h1>H</h1><blockquote><p>Quoted.</p></blockquote><ul><li>Item one</li><li>Item two</li></ul><ol><li>First</li><li>Second</li></ol>",
    )
    .expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(
        eps[0].text,
        "Quoted.\n\nItem one\n\nItem two\n\nFirst\n\nSecond"
    );
}

/// A blockquote containing multiple paragraphs keeps the paragraph
/// boundaries of its <p> children (bajan-dp2).
#[test]
fn html_blockquote_paragraphs_split() {
    let eps = html_episodes("<h1>H</h1><blockquote><p>One.</p><p>Two.</p></blockquote>")
        .expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].text, "One.\n\nTwo.");
}

/// Table cells and definition-list terms are paragraph boundaries too
/// (bajan-dp2).
#[test]
fn html_table_cells_and_definitions_separate() {
    let eps = html_episodes(
        "<h1>H</h1><table><tr><td>Alpha</td><td>Beta</td></tr></table><dl><dt>Term</dt><dd>Def</dd></dl>",
    )
    .expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].text, "Alpha\n\nBeta\n\nTerm\n\nDef");
}

/// Whitespace-only segments produce no episodes (ic_malformed would reject
/// them at ingest; the converter never emits them).
#[test]
fn html_whitespace_only_produces_nothing() {
    let eps = html_episodes("<h1></h1><p>   </p>").expect("converts");
    assert!(eps.is_empty());
}

// --- empty input ---------------------------------------------------------

/// Empty input is a valid empty stream (ic_empty_stream is a no-op, never
/// an error).
#[test]
fn empty_inputs_yield_empty_streams() {
    assert!(markdown_episodes("").expect("converts").is_empty());
    assert!(html_episodes("").expect("converts").is_empty());
}

// --- full JSON round-trip ------------------------------------------------

/// The serialized stream (what the bins print) parses back as the exact
/// records and persists through bajan — the literal converter → ingest
/// pipe.
#[test]
fn serialized_stream_round_trips_through_bajan() {
    for eps in [
        markdown_episodes("# A\n\nAlpha.\n\n## B\n\nBeta.\n").expect("converts"),
        html_episodes("<h1>A</h1><p>Alpha.</p><h2>B</h2><p>Beta.</p>").expect("converts"),
    ] {
        let json = serde_json::to_string(&eps).expect("serializes");
        let parsed: Vec<EpisodeRecord> = serde_json::from_str(&json).expect("schema-valid");
        assert_eq!(parsed, eps);
        let outcomes = persist_all(&parsed);
        assert_eq!(outcomes.len(), eps.len());
        assert!(outcomes.iter().all(|o| outcome_of(o) == "persisted"));
    }
}

use proptest::prelude::*;

// Stream-wide id uniqueness over arbitrary heading corpora: any sequence
// of (level, anchor-text) headings with a body must emit pairwise-distinct
// ids (bajan-a4d regression property).
proptest! {
    #[test]
    fn p_stream_ids_unique(headings in proptest::collection::vec(
        proptest::arbitrary::any::<u8>().prop_map(|n| {
            let level = n % 6 + 1;
            let word = ["Notes", "notes", "NOTES", "Notes 2", "A B", "a-b", "section", "", "Übung", "over the  lazy dog"][n as usize % 10];
            (level, word.to_string())
        }),
        0..20,
    )) {
        let mut md = String::new();
        for (level, word) in &headings {
            md.push_str(&format!("{} {}\n\nBody {}.\n\n", "#".repeat(*level as usize), word, word));
        }
        let eps = bajan_converters::markdown_episodes(&md).expect("converts");
        let ids: std::collections::HashSet<&str> = eps.iter().map(|e| e.id.as_str()).collect();
        prop_assert_eq!(ids.len(), eps.len(), "duplicate ids: {:?}", eps.iter().map(|e| &e.id).collect::<Vec<_>>());
    }
}

// --- epub mapping (bajan-buy) ---------------------------------------------

/// Minimal hand-rolled stored-zip writer — the EPUB fixture is constructed
/// in-test (no binary fixture in the repo). CRC32 included so real readers
/// (ZipArchive) verify the entries.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

struct ZipBuilder {
    out: Vec<u8>,
    entries: Vec<(String, u32, u32, u32)>, // name, crc, size, offset
}

impl ZipBuilder {
    fn new() -> Self {
        ZipBuilder {
            out: Vec::new(),
            entries: Vec::new(),
        }
    }
    fn add(&mut self, name: &str, data: &[u8]) {
        let offset = self.out.len() as u32;
        let crc = crc32(data);
        self.out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        self.out.extend_from_slice(&20_u16.to_le_bytes()); // version needed
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // flags
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // method: stored
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // mod time
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // mod date
        self.out.extend_from_slice(&crc.to_le_bytes());
        self.out
            .extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.out
            .extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.out
            .extend_from_slice(&(name.len() as u16).to_le_bytes());
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // extra len
        self.out.extend_from_slice(name.as_bytes());
        self.out.extend_from_slice(data);
        self.entries
            .push((name.to_string(), crc, data.len() as u32, offset));
    }
    fn finish(mut self) -> Vec<u8> {
        let cd_start = self.out.len() as u32;
        for (name, crc, size, offset) in &self.entries {
            self.out.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
            self.out.extend_from_slice(&20_u16.to_le_bytes()); // version made by
            self.out.extend_from_slice(&20_u16.to_le_bytes()); // version needed
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // flags
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // method
            self.out.extend_from_slice(&0_u16.to_le_bytes());
            self.out.extend_from_slice(&0_u16.to_le_bytes());
            self.out.extend_from_slice(&crc.to_le_bytes());
            self.out.extend_from_slice(&size.to_le_bytes());
            self.out.extend_from_slice(&size.to_le_bytes());
            self.out
                .extend_from_slice(&(name.len() as u16).to_le_bytes());
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // extra
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // comment
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // disk
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // internal attrs
            self.out.extend_from_slice(&0_u32.to_le_bytes()); // external attrs
            self.out.extend_from_slice(&offset.to_le_bytes());
            self.out.extend_from_slice(name.as_bytes());
        }
        let cd_len = self.out.len() as u32 - cd_start;
        self.out.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // disk
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // cd disk
        self.out
            .extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        self.out
            .extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        self.out.extend_from_slice(&cd_len.to_le_bytes());
        self.out.extend_from_slice(&cd_start.to_le_bytes());
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // comment len
        self.out
    }
}

fn xhtml(title: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{title}</title></head>\n\
         <body>{body}</body></html>"
    )
}

/// Three-chapter fixture book: toc labels c1/c2 only; c3 has no toc entry
/// and no heading (fallback slug); date is a clean civil date.
fn build_fixture_epub(date: &str) -> Vec<u8> {
    let opf = format!(
        "<?xml version=\"1.0\"?>\n\
         <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"uid\">\n\
         <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
         <dc:identifier id=\"uid\">urn:uuid:fixture</dc:identifier>\n\
         <dc:title>Fixture Book</dc:title>\n\
         <dc:creator>Tester</dc:creator>\n\
         <dc:date>{date}</dc:date>\n\
         <dc:language>en</dc:language>\n\
         </metadata>\n\
         <manifest>\n\
         <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n\
         <item id=\"c1\" href=\"c1.xhtml\" media-type=\"application/xhtml+xml\"/>\n\
         <item id=\"c2\" href=\"c2.xhtml\" media-type=\"application/xhtml+xml\"/>\n\
         <item id=\"c3\" href=\"c3.xhtml\" media-type=\"application/xhtml+xml\"/>\n\
         </manifest>\n\
         <spine><itemref idref=\"c1\"/><itemref idref=\"c2\"/><itemref idref=\"c3\"/></spine>\n\
         </package>"
    );
    let nav = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
               <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Nav</title></head>\n\
               <body><nav epub:type=\"toc\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n\
               <ol><li><a href=\"c1.xhtml\">Intro</a></li><li><a href=\"c2.xhtml\">Deep Dive</a></li></ol>\n\
               </nav></body></html>";
    let container = "<?xml version=\"1.0\"?>\n\
                     <container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n\
                     <rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles>\n\
                     </container>";
    let mut z = ZipBuilder::new();
    z.add("mimetype", b"application/epub+zip");
    z.add("META-INF/container.xml", container.as_bytes());
    z.add("OEBPS/content.opf", opf.as_bytes());
    z.add("OEBPS/nav.xhtml", nav.as_bytes());
    z.add(
        "OEBPS/c1.xhtml",
        xhtml(
            "Intro",
            "<h1>Introduction</h1><p>Welcome to the fixture book.</p><p>Second paragraph.</p>",
        )
        .as_bytes(),
    );
    z.add(
        "OEBPS/c2.xhtml",
        xhtml("Ch2", "<h2>Deep Dive</h2><p>Chapter two body text.</p>").as_bytes(),
    );
    z.add(
        "OEBPS/c3.xhtml",
        xhtml(
            "Ch3",
            "<p>No headings here, just prose for the fallback id.</p>",
        )
        .as_bytes(),
    );
    z.finish()
}

/// Chapter splitting: each spine document is one (or more) episodes; three
/// content chapters yield three episodes, nav/opf never become episodes.
#[test]
fn epub_chapter_split_count() {
    let bytes = build_fixture_epub("2023-05-17");
    let eps = bajan_converters::epub_episodes(&bytes).expect("converts");
    assert_eq!(
        eps.len(),
        3,
        "got: {:?}",
        eps.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
}

/// Chapter titles: toc label wins for both the slug id ("Intro" → intro)
/// and the heading: locator — the toc is the book's own chapter naming. The
/// fallback chapter (no toc entry, no heading) gets the position-based
/// chapter-NNN slug with the typed absent locator (never a invented
/// heading anchor), prefixed by the book slug.
#[test]
fn epub_toc_label_slug_but_heading_anchor() {
    let bytes = build_fixture_epub("2023-05-17");
    let eps = bajan_converters::epub_episodes(&bytes).expect("converts");
    assert_eq!(eps[0].id, "fixture-book-intro");
    assert_eq!(eps[0].locator, Locator::Span("heading:Intro".into()));
    assert_eq!(
        eps[0].text,
        "Welcome to the fixture book.\n\nSecond paragraph."
    );
    assert_eq!(eps[1].id, "fixture-book-deep-dive");
    assert_eq!(eps[1].locator, Locator::Span("heading:Deep Dive".into()));
    // Fallback: no toc label, no heading → chapter-NNN (spine position).
    assert_eq!(eps[2].id, "fixture-book-chapter-003");
    assert_eq!(eps[2].locator, Locator::Absent);
}

/// Metadata mapping: source_type 'epub', dc:date as a civil date becomes
/// data_cutoff, authority_tier 3, no tags (ic_date_fidelity: present only
/// because the source states it).
#[test]
fn epub_metadata_mapping() {
    let bytes = build_fixture_epub("2023-05-17");
    let eps = bajan_converters::epub_episodes(&bytes).expect("converts");
    for ep in &eps {
        assert_eq!(ep.source.source_type, "epub");
        assert_eq!(ep.source.data_cutoff.as_deref(), Some("2023-05-17"));
        assert_eq!(ep.source.authority_tier, 3);
        assert!(ep.source.tags.is_empty());
    }
}

/// A dc:date that is not a clean civil date is ABSENT, never coerced
/// (ic_date_fidelity: never invent a date).
#[test]
fn epub_date_unparseable_is_absent() {
    let bytes = build_fixture_epub("May 2023");
    let eps = bajan_converters::epub_episodes(&bytes).expect("converts");
    assert!(eps.iter().all(|e| e.source.data_cutoff.is_none()));
}

/// Same file twice → identical streams (stable ids are the identity bajan's
/// resubmission policy keys on).
#[test]
fn epub_deterministic_same_file_twice() {
    let bytes = build_fixture_epub("2023-05-17");
    let a = bajan_converters::epub_episodes(&bytes).expect("converts");
    let b = bajan_converters::epub_episodes(&bytes).expect("converts");
    assert_eq!(a, b);
}

/// The converted book persists through the real ingest path, all-persisted.
#[test]
fn epub_round_trip_persist_all_persisted() {
    let bytes = build_fixture_epub("2023-05-17");
    let eps = bajan_converters::epub_episodes(&bytes).expect("converts");
    let outcomes = persist_all(&eps);
    assert_eq!(outcomes.len(), eps.len());
    assert!(outcomes.iter().all(|o| outcome_of(o) == "persisted"));
}

/// Corrupt input (not a zip, not an epub) fails honestly: an error, never a
/// partial or garbage stream.
#[test]
fn epub_corrupt_input_fails_honestly() {
    let err = bajan_converters::epub_episodes(b"this is not a zip file at all")
        .expect_err("corrupt input must error");
    assert!(!err.is_empty());
    // A valid zip that is not an EPUB (no container.xml) also errors.
    let mut z = ZipBuilder::new();
    z.add("random.txt", b"hello");
    let err = bajan_converters::epub_episodes(&z.finish()).expect_err("non-epub zip must error");
    assert!(!err.is_empty());
}

/// EPUB2 books carry toc.ncx (which the epub crate parses into doc.toc);
/// labels resolve the same way. Chapter with a heading but NO toc entry
/// falls back to the first heading's text for both slug and locator.
#[test]
fn epub_ncx_labels_and_heading_fallback() {
    let opf = "<?xml version=\"1.0\"?>\n\
               <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"2.0\" unique-identifier=\"uid\">\n\
               <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
               <dc:identifier id=\"uid\">urn:uuid:fixture2</dc:identifier>\n\
               <dc:title>Legacy Book</dc:title>\n\
               <dc:language>en</dc:language>\n\
               </metadata>\n\
               <manifest>\n\
               <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx\"/>\n\
               <item id=\"c1\" href=\"text/c1.xhtml\" media-type=\"application/xhtml+xml\"/>\n\
               <item id=\"c2\" href=\"text/c2.xhtml\" media-type=\"application/xhtml+xml\"/>\n\
               </manifest>\n\
               <spine toc=\"ncx\"><itemref idref=\"c1\"/><itemref idref=\"c2\"/></spine>\n\
               </package>";
    let ncx = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
               <ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n\
               <head/><docTitle><text>Legacy Book</text></docTitle>\n\
               <navMap>\n\
               <navPoint id=\"n1\" playOrder=\"1\"><navLabel><text>First</text></navLabel>\n\
               <content src=\"text/c1.xhtml\"/></navPoint>\n\
               </navMap>\n\
               </ncx>";
    let container = "<?xml version=\"1.0\"?>\n\
                     <container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n\
                     <rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles>\n\
                     </container>";
    let mut z = ZipBuilder::new();
    z.add("mimetype", b"application/epub+zip");
    z.add("META-INF/container.xml", container.as_bytes());
    z.add("OEBPS/content.opf", opf.as_bytes());
    z.add("OEBPS/toc.ncx", ncx.as_bytes());
    z.add(
        "OEBPS/text/c1.xhtml",
        xhtml("C1", "<h1>Alpha</h1><p>One.</p>").as_bytes(),
    );
    z.add(
        "OEBPS/text/c2.xhtml",
        xhtml("C2", "<h3>Beta</h3><p>Two.</p>").as_bytes(),
    );
    let bytes = z.finish();

    let eps = bajan_converters::epub_episodes(&bytes).expect("converts");
    assert_eq!(eps.len(), 2);
    // c1: ncx label "First" wins over the heading "Alpha".
    assert_eq!(eps[0].id, "legacy-book-first");
    assert_eq!(eps[0].locator, Locator::Span("heading:First".into()));
    // c2: no ncx entry → first heading fallback.
    assert_eq!(eps[1].id, "legacy-book-beta");
    assert_eq!(eps[1].locator, Locator::Span("heading:Beta".into()));
    assert_eq!(
        eps[0].source.data_cutoff, None,
        "no dc:date → absent cutoff"
    );
}

// --- pdf mapping (bajan-74r) -----------------------------------------------

/// Hand-rolled minimal PDF fixture: N pages, one Helvetica text object per
/// page (Tj), plus an optional /Info dictionary. Built in-test — no binary
/// fixture in the repo.
fn build_fixture_pdf(pages_texts: &[&str]) -> Vec<u8> {
    build_fixture_pdf_with_info(pages_texts, None)
}

fn build_fixture_pdf_with_info(pages_texts: &[&str], info: Option<(&str, &str)>) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    let n = pages_texts.len();
    let mut kids = String::new();
    let mut page_obj_nums = Vec::new();
    let mut content_obj_nums = Vec::new();
    let mut obj_num = 3usize;
    for _ in pages_texts {
        kids.push_str(&format!("{obj_num} 0 R "));
        page_obj_nums.push(obj_num);
        content_obj_nums.push(obj_num + 1);
        obj_num += 2;
    }
    let font_num = obj_num;
    objects.push(Vec::new()); // placeholder for object 0
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.trim()).into_bytes());
    for (i, text) in pages_texts.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R /Resources << /Font << /F1 {font_num} 0 R >> >> >>",
                content_obj_nums[i]
            )
            .into_bytes(),
        );
        let stream = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET").into_bytes();
        let mut content = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        content.extend_from_slice(&stream);
        content.extend_from_slice(b"\nendstream");
        objects.push(content);
    }
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
    let info_num = if info.is_some() { obj_num + 1 } else { 0 };
    if let Some((title, author)) = info {
        objects.push(format!("<< /Title ({title}) /Author ({author}) >>").into_bytes());
    }

    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0u32; objects.len()];
    for (num, body) in objects.iter().enumerate().skip(1) {
        offsets[num] = out.len() as u32;
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_pos = out.len() as u32;
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len()).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets[1..] {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    let info_ref = if info.is_some() {
        format!(" /Info {info_num} 0 R")
    } else {
        String::new()
    };
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R{} >>\nstartxref\n{}\n%%EOF\n",
            objects.len(),
            info_ref,
            xref_pos
        )
        .as_bytes(),
    );
    out
}

/// Page-per-episode: each page is one episode with a `page:N` span locator
/// (1-based), text whitespace-collapsed (ic_verbatim-sanctioned page
/// locators).
#[test]
fn pdf_page_per_episode() {
    let bytes = build_fixture_pdf(&[
        "Hello page one.",
        "Second page text here.",
        "Final page prose.",
    ]);
    let eps = bajan_converters::pdf_episodes(&bytes).expect("converts");
    assert_eq!(eps.len(), 3);
    assert_eq!(eps[0].locator, Locator::Span("page:1".into()));
    assert_eq!(eps[0].text, "Hello page one.");
    assert_eq!(eps[1].locator, Locator::Span("page:2".into()));
    assert_eq!(eps[2].locator, Locator::Span("page:3".into()));
    assert_eq!(eps[2].text, "Final page prose.");
}

/// Document metadata (/Info title/author) never becomes episodes — the
/// head-leak lesson (bajan-tx4) at container level.
#[test]
fn pdf_metadata_never_episodes() {
    let bytes = build_fixture_pdf_with_info(
        &["Real page content."],
        Some(("Fixture Meta Book", "Nobody")),
    );

    let eps = bajan_converters::pdf_episodes(&bytes).expect("converts");
    assert_eq!(
        eps.len(),
        1,
        "{:?}",
        eps.iter().map(|e| &e.text).collect::<Vec<_>>()
    );
    assert!(!eps[0].text.contains("Fixture Meta Book"));
    assert!(!eps[0].text.contains("Nobody"));
    assert_eq!(eps[0].text, "Real page content.");
}

/// Empty pages emit nothing; following pages keep their true page numbers
/// (page:N is the source position, never renumbered).
#[test]
fn pdf_empty_pages_emit_nothing() {
    let bytes = build_fixture_pdf(&["", "Only page two has text."]);
    let eps = bajan_converters::pdf_episodes(&bytes).expect("converts");
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].locator, Locator::Span("page:2".into()));
}

/// bajan-98x: when N>0 pages ALL yield no text after collapse, that is a
/// probable extractor encoding failure (pdf-extract silently returns empty
/// text for Type0/Identity-H CID fonts) — pdf2bajan must exit 1 with a
/// diagnostic naming the page count, never a silent empty stream.
#[test]
fn pdf_all_pages_textless_is_error_naming_page_count() {
    let bytes = build_fixture_pdf(&["   ", "\n"]);
    let err = bajan_converters::pdf_episodes(&bytes).expect_err("all-textless pages must error");
    assert!(
        err.contains("2"),
        "diagnostic must name the page count: {err}"
    );
}

/// bajan-98x: a 0-page document is an honest empty stream — Ok, exit 0.
#[test]
fn pdf_zero_pages_is_honest_empty_stream() {
    let bytes = build_fixture_pdf(&[]);
    let eps = bajan_converters::pdf_episodes(&bytes).expect("0-page doc converts");
    assert!(eps.is_empty());
}

/// The extractor-version caveat is ON the record: every episode carries
/// source_type 'pdf' and a tags entry naming the extractor crate+version,
/// so a version change is visible per-record. Cutoff absent (ic_date_fidelity).
#[test]
fn pdf_extractor_version_tag_on_every_record() {
    let bytes = build_fixture_pdf(&["One.", "Two."]);
    let eps = bajan_converters::pdf_episodes(&bytes).expect("converts");
    for ep in &eps {
        assert_eq!(ep.source.source_type, "pdf");
        assert_eq!(ep.source.data_cutoff, None);
        assert_eq!(ep.source.authority_tier, 3);
        assert!(
            ep.source
                .tags
                .iter()
                .any(|t| t.starts_with("pdf-extractor:pdf-extract-")),
            "missing extractor tag: {:?}",
            ep.source.tags
        );
    }
}

/// The tag names the version the Cargo.toml actually pins — the const and
/// the pin must co-evolve or the tag lies.
#[test]
fn pdf_extractor_tag_matches_pinned_version() {
    let cargo = include_str!("../Cargo.toml");
    let pin = cargo
        .lines()
        .find_map(|l| l.trim().strip_prefix("pdf-extract = \""))
        .and_then(|s| s.split('"').next())
        .expect("pdf-extract pin in Cargo.toml");
    let bytes = build_fixture_pdf(&["One."]);
    let eps = bajan_converters::pdf_episodes(&bytes).expect("converts");
    let expected = format!("pdf-extractor:pdf-extract-{pin}");
    assert!(
        eps[0].source.tags.contains(&expected),
        "tag {:?} != pinned {pin}",
        eps[0].source.tags
    );
}

/// Same file twice → identical streams (same-version determinism; the
/// resubmission policy keys on these ids).
#[test]
fn pdf_deterministic_same_version() {
    let bytes = build_fixture_pdf(&["One.", "Two."]);
    let a = bajan_converters::pdf_episodes(&bytes).expect("converts");
    let b = bajan_converters::pdf_episodes(&bytes).expect("converts");
    assert_eq!(a, b);
}

/// Converted pages persist through the real ingest path, all-persisted.
#[test]
fn pdf_round_trip_persist_all_persisted() {
    let bytes = build_fixture_pdf(&["Alpha page.", "Beta page."]);
    let eps = bajan_converters::pdf_episodes(&bytes).expect("converts");
    let outcomes = persist_all(&eps);
    assert_eq!(outcomes.len(), eps.len());
    assert!(outcomes.iter().all(|o| outcome_of(o) == "persisted"));
}

/// Corrupt input fails honestly: an error, never a partial or garbage
/// stream.
#[test]
fn pdf_corrupt_input_fails_honestly() {
    let err =
        bajan_converters::pdf_episodes(b"this is not a pdf").expect_err("corrupt input must error");
    assert!(!err.is_empty());
}

// --- RO5U round-2 fixes (bajan-87z, bajan-80x, bajan-8cg) ------------------

/// PDF with a dangling object reference: the font object is referenced by
/// /Resources but absent from the body — pdf-extract PANICS on this class
/// (missing object reference), so pdf_episodes must catch the panic and
/// return an honest error instead (bajan-87z). Before the fix this test
/// aborts the runner.
#[test]
fn pdf_dangling_object_reference_is_error_not_panic() {
    let content1 = b"BT /F1 12 Tf 72 720 Td (Hello) Tj ET";
    let mut objects: Vec<Vec<u8>> = vec![Vec::new()];
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.push(
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
    );
    let mut content = format!("<< /Length {} >>\nstream\n", content1.len()).into_bytes();
    content.extend_from_slice(content1);
    content.extend_from_slice(b"\nendstream");
    objects.push(content);
    // Object 5 (the font) deliberately absent from the body AND the xref.
    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0u32; objects.len()];
    for (num, body) in objects.iter().enumerate().skip(1) {
        offsets[num] = out.len() as u32;
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_pos = out.len() as u32;
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len()).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets[1..] {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len(),
            xref_pos
        )
        .as_bytes(),
    );

    let err = bajan_converters::pdf_episodes(&out).expect_err("dangling ref must error");
    assert!(!err.is_empty());
}

/// A chapter with invalid UTF-8 bytes (a valid EPUB 2 windows-1252 book)
/// must never be SILENTLY dropped: the text survives with U+FFFD marks on
/// the record (bajan-80x), or the whole conversion errors — never a
/// silently shorter stream.
#[test]
fn epub_non_utf8_chapter_is_marked_not_silent() {
    // Rebuild a two-chapter book directly: c2 carries raw latin-1 bytes.
    let mut z = ZipBuilder::new();
    z.add("mimetype", b"application/epub+zip");
    z.add(
        "META-INF/container.xml",
        b"<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>",
    );
    z.add(
        "content.opf",
        b"<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"uid\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"uid\">u</dc:identifier><dc:title>Bad Enc</dc:title><dc:language>en</dc:language></metadata><manifest><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/><item id=\"c1\" href=\"c1.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"c2\" href=\"c2.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"c1\"/><itemref idref=\"c2\"/></spine></package>",
    );
    z.add(
        "nav.xhtml",
        b"<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><body><nav epub:type=\"toc\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><ol><li><a href=\"c1.xhtml\">Alpha</a></li><li><a href=\"c2.xhtml\">Beta</a></li></ol></nav></body></html>",
    );
    z.add(
        "c1.xhtml",
        xhtml("A", "<h1>Alpha</h1><p>Fine chapter.</p>").as_bytes(),
    );
    let mut ch2 = b"<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><body><h1>Beta</h1><p>caf".to_vec();
    ch2.push(0xE9); // latin-1 'é' — invalid UTF-8
    ch2.extend_from_slice(b" r\xE9sum\xE9.</p></body></html>");
    z.add("c2.xhtml", &ch2);
    let bytes = z.finish();

    let eps = bajan_converters::epub_episodes(&bytes).expect("converts");
    assert_eq!(
        eps.len(),
        2,
        "both chapters must appear: {:?}",
        eps.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
    let beta = &eps[1];
    assert!(
        beta.text.contains("caf\u{FFFD}"),
        "damage must be marked on the record, got {:?}",
        beta.text
    );
}

/// civil_date validates month length: Feb 30 impossible, Feb 29 only in
/// leap years, Apr 31 impossible (bajan-8cg). Exercised through dc:date.
#[test]
fn civil_date_month_length_enforced() {
    for (date, expected) in [
        ("2023-02-30", None),
        ("2024-02-29", Some("2024-02-29")),
        ("2023-02-29", None),
        ("2023-04-31", None),
        ("2000-02-29", Some("2000-02-29")), // divisible by 400 → leap
        ("1900-02-29", None),               // divisible by 100 but not 400
        ("2023-12-31", Some("2023-12-31")),
    ] {
        let bytes = build_fixture_epub(date);
        let eps = bajan_converters::epub_episodes(&bytes).expect("converts");
        let got = eps[0].source.data_cutoff.clone();
        assert_eq!(got, expected.map(str::to_string), "dc:date {date}");
    }
}
