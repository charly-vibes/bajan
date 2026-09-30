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

/// Deterministic id from the anchor (or fallback) + occurrence index:
/// same input always yields the same ids (stable ids are what bajan's
/// resubmission/idempotence policy is keyed on).
fn segment_id(anchor: Option<&str>, index: usize) -> String {
    let slug: String = match anchor {
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
    };
    // Disambiguate repeated anchors deterministically: `slug`, `slug-2`, …
    if index == 0 {
        slug
    } else {
        format!("{slug}-{}", index + 1)
    }
}

/// Finish a segment into an episode record — drops empty ones so bajan
/// never sees a whitespace-only episode (ic_malformed is bajan's guard;
/// converters don't emit records it must reject).
fn finish(seg: &Seg, index: usize, source_type: &str) -> Option<EpisodeRecord> {
    let text = seg.text();
    if text.is_empty() {
        return None;
    }
    Some(EpisodeRecord {
        id: segment_id(seg.anchor.as_deref(), index),
        text,
        locator: seg.locator.clone(),
        source: default_source(source_type),
    })
}

/// Collect finished segments into episode records, assigning ids with
/// per-anchor occurrence counters for disambiguation.
fn collect(segs: &[Seg], source_type: &str) -> Vec<EpisodeRecord> {
    let mut episodes = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for seg in segs {
        let key = seg.anchor.clone().unwrap_or_default();
        let idx = seen.entry(key).or_insert(0);
        let id_index = *idx;
        *idx += 1;
        if let Some(ep) = finish(seg, id_index, source_type) {
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

/// Convert an HTML document into episode records: `h1`–`h6` elements open
/// anchored episodes, `<p>` elements are paragraphs, all other text
/// content joins the current one. `<script>`, `<style>`, and `<pre>`
/// subtrees are excluded — they are code, not source prose.
pub fn html_episodes(input: &str) -> Result<Vec<EpisodeRecord>, String> {
    use scraper::{ElementRef, Html, Node, Selector};

    let doc = Html::parse_document(input);
    let heading_sel = Selector::parse("h1,h2,h3,h4,h5,h6").expect("valid selector");
    let para_sel = Selector::parse("p").expect("valid selector");
    let skip_sel = Selector::parse("script,style,pre").expect("valid selector");

    let mut segs: Vec<Seg> = vec![Seg::new(Locator::Absent, None)];

    fn walk(
        el: ElementRef,
        segs: &mut Vec<Seg>,
        heading_sel: &Selector,
        para_sel: &Selector,
        skip_sel: &Selector,
    ) {
        if skip_sel.matches(&el) {
            return;
        }
        if heading_sel.matches(&el) {
            // Flush the in-flight paragraph of the current segment, then
            // open a new segment anchored on the heading text (which is
            // the anchor, never episode content).
            if let Some(last) = segs.last_mut() {
                last.flush();
            }
            let anchor = collapse(&el.text().collect::<String>());
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
        } else {
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
                walk(child, segs, heading_sel, para_sel, skip_sel);
            }
        }
    }

    walk(
        doc.root_element(),
        &mut segs,
        &heading_sel,
        &para_sel,
        &skip_sel,
    );

    Ok(collect(&segs, "html"))
}
