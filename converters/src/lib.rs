//! Purpose: shared conversion core for the upstream converters — turn
//! markdown or HTML source into a bajan episode stream
//! (`EpisodeRecord`s). Responsibilities: heading-anchored segmenting
//! (each heading opens an episode keyed on a `heading:<text>` locator),
//! typed absent marker when no heading precedes content, paragraph
//! structure preserved as `\n\n` joins, deterministic slug ids with `-N`
//! disambiguation, code exclusions (script/style/pre, fenced blocks).
//! Rationale: converters live UPSTREAM of bajan (bajan-quq; the
//! bajan-15i converter table) — they do the format parsing so bajan
//! never has to (ic_no_format_parsing); everything emitted conforms to
//! the published episode-stream schema by construction, because the
//! records are bajan's own `EpisodeRecord` type via a path dep.

use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use scraper::{ElementRef, Node, Selector};

use std::collections::HashMap;

/// Default source metadata stamped on every converted episode. The
/// converter cannot know a source's authority or cutoff — it leaves the
/// fields at their honest defaults (absent cutoff per ic_date_fidelity:
/// never invent a date; tier 3 = lowest authority) for the submitting
/// operator to override before ingest.
pub fn default_source(source_type: &str) -> SourceMeta {
    SourceMeta {
        source_type: source_type.to_string(),
        data_cutoff: None,
        authority_tier: 3,
        tags: vec![],
    }
}

/// Collapse runs of whitespace and trim.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One pending episode under construction: paragraphs accumulated since
/// the last heading.
struct Seg {
    locator: Locator,
    /// Heading text (when any) — feeds the slug id.
    anchor: Option<String>,
    /// Completed paragraphs (each whitespace-collapsed).
    paras: Vec<String>,
    /// Paragraph currently being accumulated.
    cur: String,
}

impl Seg {
    fn new(locator: Locator, anchor: Option<String>) -> Self {
        Seg {
            locator,
            anchor,
            paras: Vec::new(),
            cur: String::new(),
        }
    }

    fn flush(&mut self) {
        let para = collapse(&self.cur);
        if !para.is_empty() {
            self.paras.push(para);
        }
        self.cur.clear();
    }

    /// Episode text: paragraphs joined by a blank line; a trailing
    /// in-flight paragraph is flushed first. Empty segments produce "".
    fn text(&self) -> String {
        let mut paras = self.paras.clone();
        let last = collapse(&self.cur);
        if !last.is_empty() {
            paras.push(last);
        }
        paras.join("\n\n")
    }
}

/// Slug base from the anchor (or fallback): lowercased, non-slug
/// characters mapped to '-', trimmed; empty slugs become `section` and
/// anchor-less segments `untitled`. Deterministic — same anchor, same
/// slug (stable ids are what bajan's resubmission/idempotence policy is
/// keyed on).
fn slug_base(anchor: Option<&str>) -> String {
    match anchor {
        Some(a) => {
            let collapsed = collapse(a).to_lowercase();
            let slug: String = collapsed
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            let trimmed = slug.trim_matches('-').to_string();
            if trimmed.is_empty() {
                "section".to_string()
            } else {
                trimmed
            }
        }
        None => "untitled".to_string(),
    }
}

/// Finish a segment into an episode record under the given (already
/// disambiguated) id — drops empty ones so bajan never sees a
/// whitespace-only episode (ic_malformed is bajan's guard; converters
/// don't emit records it must reject).
fn finish(seg: &Seg, id: String, source_type: &str) -> Option<EpisodeRecord> {
    let text = seg.text();
    if text.is_empty() {
        return None;
    }
    Some(EpisodeRecord {
        id,
        text,
        locator: seg.locator.clone(),
        source: default_source(source_type),
    })
}

/// Collect finished segments into episode records with ids that are
/// unique within the emitted stream (bajan-a4d). The disambiguation
/// loop is driven by the set of ids ALREADY ISSUED in this stream —
/// not by a per-anchor counter — so no two segments can receive the
/// same id, whatever the collision route: case-variant anchors
/// ('Notes'/'notes' → same slug), repeated anchors, or a heading whose
/// own text mimics a disambiguator form ('Notes 2' after a disambiguated
/// 'notes-2'). Deterministic: ids are assigned in segment order, so the
/// same input always yields the same ids.
fn collect(segs: &[Seg], source_type: &str) -> Vec<EpisodeRecord> {
    let mut episodes = Vec::new();
    let mut issued: std::collections::HashSet<String> = std::collections::HashSet::new();
    for seg in segs {
        let slug = slug_base(seg.anchor.as_deref());
        let mut id = slug.clone();
        let mut n = 1;
        while !issued.insert(id.clone()) {
            n += 1;
            id = format!("{slug}-{n}");
        }
        if let Some(ep) = finish(seg, id, source_type) {
            episodes.push(ep);
        }
    }
    episodes
}

/// Convert CommonMark text into episode records. Each heading (any level)
/// closes the current episode and opens a new one anchored on the heading
/// text; text before the first heading gets the typed absent locator.
/// Paragraph boundaries become `\n\n` joins; fenced code blocks are
/// excluded (code, not source prose). Links and other inline structure
/// contribute their text content — the locator contract needs the anchor,
/// not the syntax.
pub fn markdown_episodes(input: &str) -> Result<Vec<EpisodeRecord>, String> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    let mut segs: Vec<Seg> = vec![Seg::new(Locator::Absent, None)];
    let mut in_heading: Option<String> = None;
    let mut code_depth: usize = 0;

    for event in Parser::new(input) {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                in_heading = Some(String::new());
            }
            Event::End(TagEnd::Heading(_)) => {
                let anchor = collapse(&in_heading.take().unwrap_or_default());
                segs.push(Seg::new(
                    Locator::Span(format!("heading:{anchor}")),
                    Some(anchor),
                ));
            }
            Event::Start(Tag::CodeBlock(_)) => code_depth += 1,
            Event::End(TagEnd::CodeBlock) => code_depth = code_depth.saturating_sub(1),
            Event::Text(t) | Event::Code(t) => {
                if let Some(h) = in_heading.as_mut() {
                    h.push_str(&t);
                } else if code_depth == 0
                    && let Some(last) = segs.last_mut()
                {
                    last.cur.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(h) = in_heading.as_mut() {
                    h.push(' ');
                } else if let Some(last) = segs.last_mut() {
                    last.cur.push(' ');
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if let Some(last) = segs.last_mut() {
                    last.flush();
                }
            }
            Event::Rule => {
                // A thematic break closes the current paragraph and opens a
                // fresh unanchored segment under the same locator.
                if let Some(last) = segs.last_mut() {
                    last.flush();
                }
                let locator = segs
                    .last()
                    .map(|s| s.locator.clone())
                    .unwrap_or(Locator::Absent);
                segs.push(Seg::new(locator, None));
            }
            _ => {}
        }
    }

    Ok(collect(&segs, "markdown"))
}

/// Block-level container elements: each one is a paragraph boundary —
/// its content becomes its own collapsed paragraph, never glued onto
/// the surrounding text (bajan-dp2; ic_verbatim).
fn is_block_container(name: &str) -> bool {
    matches!(
        name,
        "li" | "blockquote"
            | "div"
            | "td"
            | "th"
            | "dt"
            | "dd"
            | "figcaption"
            | "section"
            | "article"
            | "aside"
            | "main"
            | "header"
            | "footer"
            | "nav"
    )
}

/// Parse a civil date (YYYY-MM-DD) strictly: two-digit month 01-12 and
/// a day that exists in that month — including the February rule (28,
/// 29 only in leap years: divisible by 4, except centuries unless by
/// 400). Anything else (natural language dates, year-only, extra text,
/// or impossible dates like 2023-02-30) is not a civil date — the caller
/// leaves data_cutoff absent (ic_date_fidelity: never invent a date).
fn civil_date(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let digits = |r: &[u8]| r.iter().all(|c| c.is_ascii_digit());
    if !(digits(&b[0..4]) && digits(&b[5..7]) && digits(&b[8..10])) {
        return None;
    }
    let year = (b[0] - b'0') as i64 * 1000
        + (b[1] - b'0') as i64 * 100
        + (b[2] - b'0') as i64 * 10
        + (b[3] - b'0') as i64;
    let month = (b[5] - b'0') as u16 * 10 + (b[6] - b'0') as u16;
    let day = (b[8] - b'0') as u16 * 10 + (b[9] - b'0') as u16;
    if !(1..=12).contains(&month) {
        return None;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_len: [u16; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=month_len[(month - 1) as usize]).contains(&day) {
        return None;
    }
    Some(s.to_string())
}

/// Append the element's descendant text, skipping subtrees matched by
/// `skip_sel` (script/style/pre) — the heading-anchor and block-leaf
/// text collector (bajan-tx4: anchors exclude embedded code). Shared by
/// the segment walker and the chapter-title fallback (bajan-buy).
fn collect_visible_text(el: ElementRef, out: &mut String, skip_sel: &Selector) {
    if skip_sel.matches(&el) {
        return;
    }
    for child in el.children() {
        match child.value() {
            Node::Text(t) => out.push_str(&t.text),
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    collect_visible_text(child_el, out, skip_sel);
                }
            }
            _ => {}
        }
    }
}

/// Shared HTML segment walker: builds the raw segments (locator + anchor +
/// paragraph text) for an HTML document. `html_episodes` collects them into
/// records directly; `epub_episodes` reuses the exact same mapping rules per
/// spine chapter document (bajan-buy) and re-ids at book level.
fn html_segments(input: &str) -> Vec<Seg> {
    use scraper::Html;

    let doc = Html::parse_document(input);
    let heading_sel = Selector::parse("h1,h2,h3,h4,h5,h6").expect("valid selector");
    let para_sel = Selector::parse("p").expect("valid selector");
    let skip_sel = Selector::parse("script,style,pre").expect("valid selector");
    let head_sel = Selector::parse("head").expect("valid selector");

    let mut segs: Vec<Seg> = vec![Seg::new(Locator::Absent, None)];

    fn walk(
        el: ElementRef,
        segs: &mut Vec<Seg>,
        heading_sel: &Selector,
        para_sel: &Selector,
        skip_sel: &Selector,
        head_sel: &Selector,
    ) {
        if skip_sel.matches(&el) || head_sel.matches(&el) {
            return;
        }
        if heading_sel.matches(&el) {
            // Flush the in-flight paragraph of the current segment, then
            // open a new segment anchored on the heading text (which is
            // the anchor, never episode content). The anchor collects the
            // heading's text EXCLUDING skip-subtrees (bajan-tx4): script
            // or style inside a heading is code, not source structure.
            if let Some(last) = segs.last_mut() {
                last.flush();
            }
            let mut anchor = String::new();
            collect_visible_text(el, &mut anchor, skip_sel);
            let anchor = collapse(&anchor);
            segs.push(Seg::new(
                Locator::Span(format!("heading:{anchor}")),
                Some(anchor),
            ));
            return;
        }
        if para_sel.matches(&el) {
            // A <p> is one paragraph: its full descendant text, collapsed.
            let para = collapse(&el.text().collect::<String>());
            if let Some(last) = segs.last_mut() {
                last.flush();
                last.cur.push_str(&para);
                last.flush();
            }
            return;
        }
        if is_block_container(el.value().name()) {
            // A block container is a paragraph boundary: its descendant
            // text becomes its own collapsed paragraph (bajan-dp2) —
            // unless it structurally contains <p> children, in which case
            // recurse so each <p> keeps its own boundary.
            if let Some(last) = segs.last_mut() {
                last.flush();
            }
            let has_para_children = el
                .children()
                .filter_map(ElementRef::wrap)
                .any(|c| para_sel.matches(&c));
            if has_para_children {
                for child in el.children().filter_map(ElementRef::wrap) {
                    walk(child, segs, heading_sel, para_sel, skip_sel, head_sel);
                }
            } else {
                let mut text = String::new();
                collect_visible_text(el, &mut text, skip_sel);
                if let Some(last) = segs.last_mut() {
                    last.cur.push_str(&collapse(&text));
                    last.flush();
                }
            }
            return;
        }
        // Only DIRECT text children flow into the current paragraph —
        // descendant text belongs to the descendant's own handler.
        for child in el.children() {
            if let Node::Text(t) = child.value()
                && let Some(last) = segs.last_mut()
            {
                last.cur.push_str(&t.text);
            }
        }
        for child in el.children().filter_map(ElementRef::wrap) {
            walk(child, segs, heading_sel, para_sel, skip_sel, head_sel);
        }
    }

    walk(
        doc.root_element(),
        &mut segs,
        &heading_sel,
        &para_sel,
        &skip_sel,
        &head_sel,
    );

    segs
}

/// First heading anchor of an HTML document, if any — the chapter-title
/// fallback for spine documents with no toc entry (bajan-buy). Head- and
/// code-subtree exclusions match the episode-text rules (bajan-tx4).
fn first_heading_anchor(input: &str) -> Option<String> {
    use scraper::Html;

    let doc = Html::parse_document(input);
    let heading_sel = Selector::parse("h1,h2,h3,h4,h5,h6").expect("valid selector");
    let skip_sel = Selector::parse("script,style,pre").expect("valid selector");
    let head_sel = Selector::parse("head").expect("valid selector");

    fn find(
        el: ElementRef,
        heading_sel: &Selector,
        skip_sel: &Selector,
        head_sel: &Selector,
    ) -> Option<String> {
        if skip_sel.matches(&el) || head_sel.matches(&el) {
            return None;
        }
        if heading_sel.matches(&el) {
            let mut text = String::new();
            collect_visible_text(el, &mut text, skip_sel);
            let text = collapse(&text);
            return if text.is_empty() { None } else { Some(text) };
        }
        for child in el.children().filter_map(ElementRef::wrap) {
            if let Some(found) = find(child, heading_sel, skip_sel, head_sel) {
                return Some(found);
            }
        }
        None
    }

    // Only element descendants can hold headings.
    for child in doc.root_element().children().filter_map(ElementRef::wrap) {
        if let Some(found) = find(child, &heading_sel, &skip_sel, &head_sel) {
            return Some(found);
        }
    }
    None
}

/// Chapter labels from the EPUB3 navigation document (the `epub` crate
/// parses only toc.ncx — its `doc.toc` is empty for EPUB3-only books, so
/// the nav doc is parsed here with the same scraper rules as the text
/// converters). Hrefs are relative to the nav document's own directory,
/// mirroring toc.ncx src resolution; fragments are stripped. Falls back to
/// the first `nav` element holding an `ol` when no `epub:type=toc` nav
/// exists. Returns href → label.
fn nav_labels(input: &str, nav_doc_dir: &std::path::Path) -> HashMap<String, String> {
    use scraper::Html;

    let doc = Html::parse_document(input);
    let nav_sel = Selector::parse("nav[type=toc]").expect("valid selector");
    let any_nav_sel = Selector::parse("nav").expect("valid selector");
    let a_sel = Selector::parse("a[href]").expect("valid selector");

    let nav_el = doc.select(&nav_sel).next().or_else(|| {
        doc.select(&any_nav_sel)
            .find(|n| n.select(&a_sel).next().is_some())
    });
    let Some(nav_el) = nav_el else {
        return HashMap::new();
    };
    nav_el
        .select(&a_sel)
        .filter_map(|a| {
            let href = a.value().attr("href")?.split('#').next()?.to_string();
            if href.is_empty() {
                return None;
            }
            let resolved = nav_doc_dir.join(&href).to_string_lossy().to_string();
            let label = collapse(&a.text().collect::<String>());
            if label.is_empty() {
                return None;
            }
            Some((resolved, label))
        })
        .collect()
}

/// Convert an HTML document into episode records: `h1`–`h6` elements open
/// anchored episodes, `<p>` elements and block containers (`li`,
/// `blockquote`, `td`, …) are paragraph boundaries, all other text
/// content joins the current one. `<script>`, `<style>`, and `<pre>`
/// subtrees are excluded — they are code, not source prose; `<head>` is
/// document metadata, never episode content (bajan-tx4).
pub fn html_episodes(input: &str) -> Result<Vec<EpisodeRecord>, String> {
    Ok(collect(&html_segments(input), "html"))
}

/// Convert an EPUB file into episode records: each spine chapter document
/// becomes one episode (chapter-split granularity — page numbers do not
/// exist in reflowable EPUB). The chapter text reuses the HTML mapping
/// rules exactly (`html_segments` per spine document: head/script/style/pre
/// excluded, block containers as paragraph boundaries, whitespace collapse)
/// with the chapter's paragraphs joined by blank lines. The chapter anchor
/// is the toc label (toc.ncx or nav doc) when one targets the chapter,
/// else the chapter's first heading text, else the position-based fallback
/// `chapter-NNN` — the fallback keeps the typed absent locator, never an
/// invented heading anchor. Episode ids are `slug(book title)-slug(chapter
/// title)`, deterministic and stream-unique (same disambiguation
/// discipline as the other converters). Metadata: `source_type` `epub`,
/// `data_cutoff` from dc:date only when it is a clean civil date
/// (ic_date_fidelity), authority_tier 3, no tags. Container failures
/// (corrupt zip, missing container, DRM-encumbered files) error honestly —
/// never a partial or garbage stream.
pub fn epub_episodes(input: &[u8]) -> Result<Vec<EpisodeRecord>, String> {
    use epub::doc::EpubDoc;
    use std::io::Cursor;

    let mut doc = EpubDoc::from_reader(Cursor::new(input.to_vec()))
        .map_err(|e| format!("failed to open EPUB container: {e:?}"))?;

    let book_prefix = doc
        .get_title()
        .map(|t| slug_base(Some(&t)))
        .unwrap_or_else(|| "book".to_string());
    let cutoff = doc.mdata("date").and_then(|m| civil_date(&m.value));

    // toc.ncx (parsed by the crate into doc.toc) and the EPUB3 nav
    // document (parsed here — the crate leaves it out) both supply chapter
    // labels, keyed on the chapter resource path sans fragment. ncx labels
    // win when both exist: the toc is the more specific navigation source.
    let mut toc_labels: HashMap<String, String> = doc
        .toc
        .iter()
        .filter_map(|np| {
            let path = np.content.to_string_lossy().split('#').next()?.to_string();
            Some((path, np.label.clone()))
        })
        .collect();
    if toc_labels.is_empty()
        && let Some(nav_id) = doc.get_nav_id()
        && let Some((nav_bytes, _)) = doc.get_resource(&nav_id)
        && let Ok(nav_html) = std::string::String::from_utf8(nav_bytes)
    {
        let nav_dir = doc
            .resources
            .get(&nav_id)
            .map(|r| r.path.parent().map(|p| p.to_path_buf()).unwrap_or_default())
            .unwrap_or_default();
        toc_labels = nav_labels(&nav_html, &nav_dir);
    }

    let mut segs: Vec<Seg> = Vec::new();
    let mut idx = 0usize;
    while let Some((bytes, mime)) = doc.get_current() {
        // The nav document and ncx are navigation metadata, not prose —
        // a book that lists them on the spine must not emit episodes
        // from them (head-leak lesson, bajan-tx4, at container level).
        let id = doc.get_current_id().unwrap_or_default();
        let is_nav = doc
            .resources
            .get(&id)
            .and_then(|r| r.properties.as_deref())
            .is_some_and(|p| p.split_ascii_whitespace().any(|t| t == "nav"))
            || mime.contains("dtbncx");
        if !is_nav && let Some(path) = doc.get_current_path() {
            // Chapters are decoded LOSSY, never gated on validity: a
            // non-UTF-8 chapter (a valid EPUB 2 windows-1252 book) must
            // appear in the stream with U+FFFD marks on the record —
            // silent content loss is the one forbidden outcome
            // (bajan-80x). html5ever handles the surrounding document
            // fine; only the undecodable bytes are replaced.
            let html = String::from_utf8_lossy(&bytes).into_owned();
            // Reuse the HTML mapping rules verbatim per spine document;
            // join the chapter's non-empty segments into one episode text.
            let paras: Vec<String> = html_segments(&html)
                .iter()
                .map(|s| s.text())
                .filter(|t| !t.is_empty())
                .collect();
            if !paras.is_empty() {
                let title = toc_labels
                    .get(path.to_string_lossy().as_ref())
                    .cloned()
                    .or_else(|| first_heading_anchor(&html));
                let (locator, anchor) = match title {
                    Some(t) => {
                        let collapsed = collapse(&t);
                        (
                            Locator::Span(format!("heading:{collapsed}")),
                            Some(collapsed),
                        )
                    }
                    None => (Locator::Absent, Some(format!("chapter-{:03}", idx + 1))),
                };
                let mut seg = Seg::new(locator, anchor);
                seg.paras = paras;
                segs.push(seg);
            }
        }
        idx += 1;
        if !doc.go_next() {
            break;
        }
    }

    let mut episodes = collect(&segs, "epub");
    for ep in &mut episodes {
        ep.id = format!("{book_prefix}-{}", ep.id);
        ep.source.data_cutoff = cutoff.clone();
    }
    Ok(episodes)
}

/// Extractor identity for the PDF converter, stamped on every emitted
/// episode as a `pdf-extractor:<crate>-<version>` tag. PDF text extraction
/// is lossy by design: reading order is a heuristic, columns scramble, and
/// extractor-version changes change output. The tag makes a version change
/// visible on the record itself, and it MUST be updated together with the
/// Cargo.toml pin (the pdf_extractor_tag_matches_pinned_version test
/// enforces the pairing). Re-converting the same PDF under a different
/// extractor version can yield different text; bajan then rejects the
/// resubmission with `conflict` (ic_mutated_resubmit) — that rejection is
/// correct behavior, protecting claims whose lineage text would otherwise
/// mutate.
pub const PDF_EXTRACTOR: &str = "pdf-extract";
pub const PDF_EXTRACTOR_VERSION: &str = "0.12.1";

/// Convert a PDF file into episode records: one episode per PAGE with the
/// locator `{"kind": "span", "value": "page:N"}` (1-based; page locators
/// are the ic_verbatim-sanctioned page anchors — reflowable-fidelity
/// paragraph locators do not exist for PDF text extraction). Page text is
/// split on blank lines into paragraphs, each whitespace-collapsed (the
/// same collapse discipline as the other converters); pages with no text
/// emit nothing but keep their page number (later pages are never
/// renumbered). Document metadata (/Info title/author) is not episode
/// content. Metadata: `source_type` `pdf`, `data_cutoff` absent
/// (ic_date_fidelity — file dates are not data cutoffs), authority_tier 3,
/// tags carry the extractor-version entry (see PDF_EXTRACTOR). Corruption
/// errors honestly — never a partial or garbage stream.
pub fn pdf_episodes(input: &[u8]) -> Result<Vec<EpisodeRecord>, String> {
    // pdf-extract panics (not Err) on some malformed-but-parseable PDFs —
    // e.g. a manifest object referenced but absent from the body — so the
    // panic is caught and surfaced as the same honest error class
    // (bajan-87z): an Err, never an abort and never a partial stream. The
    // default panic hook is suppressed for the call scope (and restored
    // after) so the abort report does not masquerade as a second failure —
    // the diagnostic travels in the returned Err instead.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let extracted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem_by_pages(input)
    }));
    std::panic::set_hook(prev_hook);
    let pages = extracted
        .map_err(|payload| {
            let detail = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "pdf extractor panicked".to_string());
            format!("failed to extract PDF text: {detail}")
        })?
        .map_err(|e| format!("failed to extract PDF text: {e:?}"))?;

    let extractor_tag = format!("pdf-extractor:{PDF_EXTRACTOR}-{PDF_EXTRACTOR_VERSION}");
    let mut source = default_source("pdf");
    source.tags.push(extractor_tag);

    let mut episodes = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        // pdf-extract's PlainTextOutput prefixes each page with "\n\n"
        // page-start markers; blank lines separate paragraphs. Collapse
        // each paragraph, drop empties, join with blank lines.
        let paras: Vec<String> = page
            .split("\n\n")
            .map(collapse)
            .filter(|p| !p.is_empty())
            .collect();
        if paras.is_empty() {
            continue;
        }
        episodes.push(EpisodeRecord {
            id: format!("page-{:03}", i + 1),
            text: paras.join("\n\n"),
            locator: Locator::Span(format!("page:{}", i + 1)),
            source: source.clone(),
        });
    }
    Ok(episodes)
}
