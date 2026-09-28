# Document-Grounded Knowledge Assistant — EARS Requirements Specification

**Status:** Reference specification. Product-independent.
**Scope:** the behaviours a **document-grounded knowledge assistant** *shall* exhibit across its
whole pipeline — ingesting source documents into a knowledge base, retrieving and ranking them,
synthesising answers, and citing provenance. Captured as testable requirements, not implementation.
**Examples are deliberately generic** (Entity-E, Activity-T, Metric-M, Event-V, Source-S,
Report-R, Scope-A/Scope-B, Reference-X) so the requirement — not any domain fact — is the thing
under test.

---

## 0. Purpose & how to read this

A **document-grounded knowledge assistant** ingests source documents (filings, presentations,
PDFs, event bulletins, web pages) into a per-workspace **knowledge base**, then answers questions
from it. The recurring, high-severity failure class for every such system is the same: **the
answer says more, fresher, or more precise than the sources support, and the user cannot trace it
back to an exact source.** This specification pins the behaviours that prevent that, across five
surfaces:

```
   ingest ──▶ knowledge base ──▶ retrieval/ranking ──▶ synthesis/answer ──▶ citations
   (write)      (storage)           (search/read)          (the gate)        (provenance)
```

Each requirement corresponds to one defect class. Defending a defect at only
one surface is insufficient — e.g. hardening *ingest* fidelity does nothing for a
number fabricated *downstream* at answer time — so requirements are grouped by surface and
cross-referenced where a single defect must be defended in depth (see Appendix B).

### System model (reference architecture)

The requirements assume this minimal architecture. Any implementation that provides these
capabilities satisfies the spec; the names are roles, not products.

- **Knowledge base** — a corpus of **pages**. Each page is a text document (e.g. Markdown) with
  two parts: a structured **metadata header** (key/value pairs — data cutoff, status, aliases,
  canonical flag, and the page's **captured evidence**) and a **body**. The body is organised
  into **source sections** (one per contributing source, each dated) and ends with a declared
  **Sources** list (the page-level roster of every source it drew from).
- **Ingest pipeline** — the write path. It parses a source document, extracts its text (for
  paginated documents, per page/slide), synthesises or updates the relevant page(s), and stamps
  the metadata header, dated section markers, and captured evidence. **Re-ingest** = re-running
  this over already-ingested sources to backfill new fields onto legacy pages.
- **Retrieval tools** — the read-side surface the agent uses: **search** (relevance-ranked
  lookup), **read** (fetch a page or section), **grep** (substring/regex over the corpus),
  **outline** (list a page's headers). These operate on the knowledge base, never on the raw
  filesystem.
- **The agent** — the conversational answering loop. A **session** is one continuous
  conversation; it contains ordered **turns**; each turn is one user request plus the agent's
  **messages** (its tool calls, tool results, and final reply) in response.
- **The gate** — the answer-time verification step (see glossary) that runs over a drafted answer
  before it reaches the user.
- **Lint** — automated batch checks run over pages (on demand or scheduled). Two distinct
  flavours exist: a **reporting lint** that only *reports* defects (temporal-health,
  contradiction, and structural-health checks) and never edits content, and a **corrective
  quality lint** that *rewrites* pages to fix stale claims, malformed metadata, and dead
  references and to mark contradictions. A requirement that relies on lint states which flavour
  it means; unqualified "lint" is the reporting flavour.

### EARS conventions used

This spec uses the **Easy Approach to Requirements Syntax**. Each requirement uses exactly one
pattern; the keyword is bolded:

| Pattern | Template | When to use |
|---|---|---|
| Ubiquitous | *The `<system>` **shall** `<response>`.* | always-on property |
| Event-driven | ***When** `<trigger>`, the `<system>` **shall** `<response>`.* | response to an event |
| State-driven | ***While** `<state>`, the `<system>` **shall** `<response>`.* | continuous, during a state |
| Unwanted | ***If** `<condition>`, **then** the `<system>` **shall** `<response>`.* | error / abuse / edge |
| Optional | ***Where** `<feature>`, the `<system>` **shall** `<response>`.* | feature-gated behaviour |

Requirement IDs are `WR-<domain>.<n>`. Each carries: the EARS statement, a one-line **rationale**,
a **generic example**, **edge cases**, and the **defect class** it defends against. Roles:
- **the ingest pipeline** — the write path that turns a source document into knowledge-base pages.
- **the agent** — the answering loop that retrieves, synthesises, and replies.
- **the system** — either or both, where the boundary is not material.

Every `shall` is **mandatory** unless the requirement is tagged *(Optional)*. Each requirement is
written to be **testable**: the generic example is the positive acceptance case, and the edge
cases enumerate the negative/boundary cases a conforming implementation must also handle. A
requirement is *met* only when both hold.

**Conformance note.** This spec has been checked against a reference implementation. Where a
requirement is realised as a **non-blocking advisory** (a warning/log) rather than a hard gate,
its body says so explicitly — that is the intended strength, not a shortfall. Requirements that
are specified but **not currently realised** by the reference implementation are collected in
Appendix D so the normative text can stay stable while conformance is tracked separately.

### Glossary

- **page / section / body** — see System model above. A **page** is one knowledge-base document;
  a **section** is one dated source-block within a page's body.
- **metadata header** — the structured key/value block at the top of a page (data cutoff, status,
  status date, aliases, canonical flag, captured evidence). In a Markdown implementation this is
  the front-matter block.
- **declared sources** — the page-level list of every source a page drew from; the fallback
  provenance floor when a specific claim cannot be pinned to one source (WR-PROV.4).
- **captured evidence** — the per-page list of verbatim quotes, each with its source name and
  locator — the exact evidence a page's claims were built from.
- **data cutoff** — the date the underlying data was locked, distinct from the ingest date and
  from "today". Held in the metadata header.
- **locator** — a resolvable pointer *into* a source: a heading anchor, a text-fragment reference,
  or a page/slide number for paginated documents.
- **the gate** — the answer-time verification step: decompose the drafted answer into claims,
  judge each claim against the retrieved evidence, then hedge / annotate / retract as needed
  before the answer reaches the user.
- **lint** — an automated batch check over pages (temporal, contradiction, or structural
  health), run on demand or on a schedule. Comes in a reporting-only and a corrective flavour
  (see §0).
- **session / turn / message** — a **session** is one continuous conversation; a **turn** is one
  user request and the agent's response to it; **messages** are the individual tool calls, tool
  results, and replies within a turn.
- **ledger** — a durable record tracking each raised contradiction flag through its lifecycle
  (open → resolved / unresolvable), as opposed to inert inline markers scattered in page text.
- **SLA** — service-level agreement: here, a time bound after which an unresolved contradiction
  flag is escalated by lint.
- **canonical page** — the single page designated authoritative when near-duplicates exist.
- **force-attach** — wrongly associating a source or claim with an entity it does not concern
  (e.g. via a loose substring/alias match).
- **stale** — data cutoff older than the configured staleness window. The window is
  **per-workspace configurable** (default: 365 days) and drives both the read-side staleness
  warning and the reporting lint.
- **source type & authority order** — the class of a source document, used for ranking. The
  default authority order, highest to lowest, is: **per-event bulletin ≈ curated briefing >
  presentation deck > formal publication**. Rationale: bulletins/briefings are issued more
  frequently and closer to events, so they usually carry the freshest data on a fast-moving fact;
  decks are periodic; generic publications lowest. Authority is a tiebreaker among
  comparably-recent sources, never a licence to surface stale data (WR-RANK.2).

---

## 1. Ingest fidelity & source grounding

*Surface: write path. Defect class: a synthesised page asserts more than its source, invents an
identity, or drops the source's hedging.*

### WR-INGEST.1 — Verbatim evidence is captured for every page
**When** the ingest pipeline writes or updates a knowledge-base page, the pipeline **shall**
persist a captured-evidence entry (verbatim quote + source name + locator) for each material
claim on the page.
- **Rationale:** a claim with no captured quote is unverifiable at answer time and cannot be
  cited. Provenance must be captured at write time; it cannot be reconstructed later.
- **Generic example:** ingesting Source-S that states *"7 of 91 units met the threshold"* yields a
  page whose captured evidence contains that exact sentence, attributed to Source-S.
- **Edge cases:** multi-line quotes must survive serialization (see WR-INGEST.5); a page built
  from N sources carries evidence from all N, not just the first.

### WR-INGEST.2 — Numeric claims must be anchored in a captured quote
**If** a numeric value on a generated page does not appear in any of that page's captured
evidence, **then** the ingest pipeline **shall** emit a grounding warning identifying the
unanchored value.
- **Rationale:** the single highest-severity defect is a fabricated number. Catch it where the
  page is written.
- **Generic example:** a page states *"success rate 44%"* but no captured quote contains `44` → a
  grounding warning fires.
- **Edge cases:** the check can over-flag (rounding, `44%` vs `44.0%`, values inside a quoted
  range) — at ingest it is an **advisory log, never a hard block**; percentages inside a quoted
  range must not false-fire.

### WR-INGEST.3 — Hedging is preserved, never upgraded
**When** the ingest pipeline synthesises page prose from a hedged source statement, the pipeline
**shall** preserve the source's epistemic strength (e.g. "signals of an effect" is not rewritten
as "demonstrated an effect").
- **Rationale:** silent confidence-inflation is a traceability failure even when the underlying
  number is right.
- **Generic example:** source says *"signals of activity were observed"* → the page must not read
  *"activity was demonstrated"*.
- **Edge cases:** estimates and internal assessments must retain their "estimate / internal
  assessment" framing (see WR-ANSWER.4).

### WR-INGEST.4 — Entity identity must not be fabricated
**If** the ingest synthesis cannot resolve an entity's true name from the source, **then** the
pipeline **shall not** invent a name or create a page under a fabricated identity.
- **Rationale:** a wrong-identity page silently poisons every downstream answer about that entity.
- **Generic example:** an entity referred to only by a short code must not be materialised as a
  full page under a guessed full name.
- **Edge cases:** known aliases (short-name ↔ code ↔ full name) must resolve to **one** canonical
  page with an alias list + redirect stubs; ambiguous abbreviations that could match multiple
  entities must not force-attach to any single one.

### WR-INGEST.5 — Metadata writes are lossless
**When** the pipeline stamps or rewrites page metadata, the pipeline **shall** preserve all
existing metadata fields and multi-line values intact.
- **Rationale:** a partial-line rewrite that orphans the remainder silently wipes provenance
  metadata — an invisible, corpus-wide data-loss class.
- **Generic example:** stamping a data-cutoff field onto a page whose captured-evidence block
  spans multiple lines must not truncate or orphan that block.
- **Edge cases:** multi-line values serialized as inline/flow syntax can become invalid and drop
  all metadata — such values must serialize as block form; a stamping routine must replace the
  whole logical field, not just its first physical line.

---

## 2. Temporal representation & recency

*Surface: write + read. Defect class: stale data returned as "current" with no date; newer data
present elsewhere never surfaced.*

### WR-TIME.1 — Every dated source section carries its data cutoff
**When** the pipeline ingests a source that carries a data cutoff or event date, the pipeline
**shall** stamp the generated section with a machine-readable "data as of `<YYYY-MM-DD>`" marker
and record the page's most-recent cutoff and latest source.
- **Rationale:** recency judgements are impossible without a machine-readable cutoff per section.
- **Generic example:** ingesting Event-V material dated 2024-07 stamps `data as of 2024-07-xx` —
  the source's date, **not** today's ingest date.
- **Edge cases:** the date is the source's cutoff, never the ingest timestamp; a source with no
  discoverable date is stamped `unknown`, not today.

### WR-TIME.2 — Reads surface recency up front
**When** the agent reads a time-sensitive page, the system **shall** present a recency banner
(data cutoff + status) at the top and order dated source sections newest-first.
- **Rationale:** the reader must see how old the data is before reading its content.
- **Generic example:** reading Activity-T's page shows a banner "data as of 2024-07; status:
  ongoing" above the body, with 2024 sections above 2023 ones.
- **Edge cases:** a legacy page with no dated headers cannot be reordered — this is a re-ingest
  data gap, not a code failure; the banner degrades gracefully.

### WR-TIME.3 — Search surfaces per-hit recency and warns on stale best-match
**When** the agent searches the knowledge base, the system **shall** surface each hit's data
cutoff, and **if** the best-scoring match is stale (older than the staleness window), **then** the
system **shall** emit a staleness warning on that hit.
- **Rationale:** relevance scoring is blind to age; a highly-relevant stale page must not read as
  current.
- **Generic example:** a query's top hit has cutoff 2023-01 (>1 year) → the result is tagged "⚠
  data may be stale (as of 2023-01)".
- **Edge cases:** a hit with no cutoff is neither warned nor claimed fresh; recency here is
  advisory to the agent, not a re-ranking (that is the optional WR-RANK.3).

### WR-TIME.4 — Generated data tables are dated
**When** the agent generates a data table from knowledge-base content, the agent **shall** caption
it with a sourced "data as of `<date>`" derived from the underlying sections' cutoffs.
- **Rationale:** a table stripped of its dates is the exact artifact that gets copied elsewhere and
  mis-read as current.
- **Generic example:** a comparison table built from three sources (2024-07, 2024-09, 2025-04) is
  captioned "data as of Apr 2025" or lists per-row cutoffs.
- **Edge cases:** mixed-cutoff tables should show per-row dates rather than a single misleading
  headline date.

### WR-TIME.5 — Answers state the cutoff and never imply post-cutoff data
**When** the agent answers a recency-sensitive question, the agent **shall** state the data cutoff
of its source, and **if** newer relevant data exists on another page, **then** the agent **shall**
surface or reconcile it rather than presenting older data as current.
- **Rationale:** the classic defect — quoting an old cutoff as "current" while a newer figure sits
  one page away.
- **Generic example:** Activity-T's own page has a 2024-07 cutoff, but Entity-E's page carries a
  2025-04 update for the same activity → the answer flags the activity page as stale and uses or
  points to the 2025-04 figure.
- **Edge cases:** if no newer data exists, say so; never fabricate recency to satisfy the ask (see
  WR-ANSWER.5).

---

## 3. Source prioritisation

*Surface: retrieval/ranking. Defect class: a low-authority or stale source outranks the freshest
authoritative one within a page.*

### WR-RANK.1 — Sections ranked by source-type authority then recency
**When** the agent orders sections within a page (or the cited sources), the system **shall** rank
by `(source_type_authority, recency)`, using the default authority order **per-event bulletin ≈
curated briefing > presentation deck > formal publication** (see glossary).
- **Rationale:** per-event bulletins fire more often than periodic decks, so they usually carry
  the freshest data on a fast-moving fact; curated briefings rank alongside them; generic
  publications lowest.
- **Generic example:** on one page, a per-event bulletin section and a generic-publication section
  cover the same fact → the bulletin ranks first.
- **Edge cases:** source type is inferred with no re-ingest (from filename pattern / extension /
  path segment); an unrecognised type maps to a **middle tier and never crashes**. This is a
  **per-section** axis, not a page-level search axis — a single page aggregates many source types.

### WR-RANK.2 — Authority never overrides staleness
**If** an otherwise-high-authority section is stale (older than the staleness window), **then** the
system **shall** demote it below fresh lower-authority sections.
- **Rationale:** a fresh deck must outrank a year-old bulletin; authority is a tiebreaker among
  comparably-recent sources, not a licence to surface stale data.
- **Generic example:** a 2023 bulletin vs a 2025 deck on the same fact → the 2025 deck ranks first.
- **Edge cases:** ranking key is `(authority_rank if not stale else demoted, recency)`.

### WR-RANK.3 *(Optional)* — Recency-weighted search ordering
**Where** recency-weighted search is enabled, the system **shall** blend relevance score with data
cutoff to order results most-recent-first. *(Currently off by default — the baseline search is
pure relevance ranking; per-hit recency and stale-best-match warnings from WR-TIME.3 carry the
recency signal instead.)*
- **Rationale:** surfaces present-but-unranked recent material (the "data is there but never
  retrieved" class).
- **Edge cases:** must be evaluation-gated so it does not regress relevance for queries where
  recency is irrelevant.

---

## 4. Provenance & citation traceability

*Surface: answer + UI. Defect class: a claim with no citation, a citation that points to the
wrong or nonexistent place, or a link that does not resolve back to the original file.*

### WR-PROV.1 — Every material numeric claim carries an inline citation
**When** the agent emits a material numeric claim in an answer or generated report, the agent
**shall** attach an inline source citation to that claim.
- **Rationale:** users must be able to trace from a number to its origin.
- **Generic example:** *"the success rate was 44% [Source-S, p.12]"* — the number and its source
  travel together.
- **Edge cases:** an unknown cell renders **"not reported"**, never a filled-in guess (ties to
  WR-ANSWER.2); qualitative claims should cite too, but numeric is the hard requirement.

### WR-PROV.2 — Citations resolve to the original source file
**When** the system renders a citation, the system **shall** link to the **original** source file,
not to a knowledge-base-internal artifact or a viewer-wrapper URL.
- **Rationale:** the user's trust anchor is the primary document, not the assistant's derived page.
- **Generic example:** a citation to a stored deck links to that deck's file (constructed from the
  repository's addressing rule); a citation to an event bulletin links to the raw source.
- **Edge cases:** viewer-wrapper URLs must be rejected in favour of the plain file URL; any
  repository/site prefix must be threaded onto absolute citation URLs; a secondary page of a
  multi-page item must still resolve to a source, not to nothing.

### WR-PROV.3 — Clicking a citation opens the source at the matched span
**When** the user clicks a citation, the system **shall** open a preview of the source located at
the matched span (heading anchor, text fragment, or page/slide).
- **Rationale:** dropping the user at the top of a 200-page document is not traceability.
- **Generic example:** clicking a claim sourced from page 65 of a paginated document opens the
  preview at page 65.
- **Edge cases:** for paginated documents the **page/slide locator takes precedence** over a text
  fragment; generated previews are disambiguated by a distinct filename suffix; page breaks in the
  extracted text drive the locator computation.

### WR-PROV.4 — Origin source resolution is deterministic and layered
**When** the gate resolves a claim's origin source, the system **shall** apply a deterministic
layered resolution: emit **all** origin sources matching the claim, fall back to the page-level
declared-sources floor when per-claim evidence does not pin one, and recompute the locator from
the resolved source.
- **Rationale:** a claim assembled from multiple sources must cite all of them; a claim with weak
  per-claim provenance must still get *a* correct page-level source, never a spill artifact.
- **Generic example:** a synthesised sentence combining two bulletins cites both; a sentence with
  no precise quote match falls back to the page's declared sources rather than citing nothing.
- **Edge cases:** raw tool-response spill artifacts must resolve to their true source, not the tool
  envelope; the page-level floor is a floor, not a ceiling (per-claim precision wins when
  available).

### WR-PROV.5 — The answer ships answer text plus claim-level provenance
**When** the gate finishes verifying an answer, the system **shall** attach to the result a list of
verified claims, each carrying its verdict, verbatim quote, source, and locator.
- **Rationale:** use cases requiring exact traceability need the answer and its full backing
  claim-set as paired, inspectable artifacts — not a flat answer-level source list.
- **Generic example:** an answer with three factual claims ships three claim-verdicts, each with
  `{label, sources:[{quote, url, locator}]}`.
- **Edge cases:** a claim carried forward from a prior turn keeps its **original** verdict verbatim
  — never silently upgraded or re-judged just because it is restated.

### WR-PROV.6 — Canonical source is unambiguous among near-duplicates
**Where** near-duplicate source pages exist, the system **shall** designate exactly one canonical
page and, when asked which to cite, the agent **shall** name it and **shall not** invent a
non-existent page.
- **Rationale:** when several near-identical pages exist, an answer can cite a page that does not
  exist — a hallucinated composite of the real ones.
- **Generic example:** three variants of a disclosures document exist → the agent cites the
  canonical one and does not fabricate a "combined analysis" page.
- **Edge cases:** the canonical flag must be durable — a re-ingest that drops it is a regression
  that makes the answer non-deterministic across reruns.

---

## 5. Answer-time faithfulness (the verification gate)

*Surface: synthesis/answer — the surface no ingest or lint check can protect. Defect class: the
model fabricates or over-asserts a value while generating the reply or report.*

### WR-ANSWER.1 — No unsourced number reaches the user
**If** a numeric value in a drafted answer or generated report appears in none of the retrieved
sources, **then** the gate **shall** flag or drop that value before it reaches the user.
- **Rationale:** the canonical failure — a fabricated table-cell overriding correct in-context
  data, happening *downstream of the knowledge base* inside a code-generated report.
- **Generic example:** every retrieved source says a rate is 0%, but the drafted comparison-table
  cell reads "7.7%" → the gate catches the unsourced `7.7`.
- **Edge cases:** the check must apply to code-execution / file-writing deliverables, **not only
  inline prose** — the hardest surface, where such fabrication typically lives. It must diff the
  draft against the raw retrieved passages (answer time has no structured captured-evidence
  objects) and be **advisory rather than a hard block** unless its false-positive rate is measured
  low.

### WR-ANSWER.2 — Unknown cells render "not reported", never invented
**When** the agent assembles a comparison table with a cell it cannot source, the agent **shall**
render that cell as "not reported" (or equivalent) rather than filling it with a plausible value.
- **Rationale:** a blank filled with a guess is indistinguishable from data to the reader.
- **Generic example:** Entity-Y's error rate is absent from all sources → the table cell reads
  "not reported", not an interpolated number.
- **Edge cases:** "0% / none observed" (a reported absence) is distinct from "not reported"
  (unknown) — the two must not be conflated.

### WR-ANSWER.3 — Hedge/annotation tags never ship raw in prose
**If** the gate annotates a claim as unverifiable, **then** the rendered answer **shall** express
it as a natural-language caveat and **shall not** leave a raw tag (e.g. "— not confirmed by
retrieved sources") glued into user-facing prose.
- **Rationale:** raw annotate-only tags leaking into shipped prose.
- **Generic example:** instead of *"Entity-A acquired Item-Z in 2025. — not confirmed by retrieved
  sources"*, the answer either omits the claim, corrects it, or reads *"(I could not confirm the
  acquisition date from the ingested sources.)"*
- **Edge cases:** applies to every annotation variant — "(unconfirmed)", "— not confirmed", etc.

### WR-ANSWER.4 — Assessments and estimates are attributed and hedged
**When** the agent states a comparative or strategic assessment (a lead time, an "ahead/behind"
judgement), the agent **shall** frame it as an estimate with source and date, **not** as
established fact.
- **Rationale:** an estimate shipped as a hard fact ("~3 years ahead").
- **Generic example:** *"per an internal assessment at Event-V (2026-05), Entity-E was estimated
  ~3+ years ahead"* — never a bare *"Entity-E is 3 years ahead"*.
- **Edge cases:** this is a **non-numeric** fabrication class — it stays prompt-governed and is
  explicitly **not** a target for the numeric grounding gate (WR-ANSWER.1).

### WR-ANSWER.5 — Absent data is flagged, never fabricated
**If** the user asks for material (a summary, a recent readout, an event record) that is not
ingested, **then** the agent **shall** state the gap explicitly and **shall not** synthesise
conclusions from adjacent/pre-event material or invent recency.
- **Rationale:** "conclusions" synthesised from a pre-event preview; a coverage gap answered from
  tangential pages.
- **Generic example:** asked for the Event-V post-event summary when only a pre-event preview is
  ingested → *"the Event-V summary has not been ingested; only pre-event material exists (dated
  …)"*.
- **Edge cases:** must **distinguish** a genuine absence from adjacent content that exists in a
  *different context* (e.g. third-party commentary about Entity-A must not be presented as an
  official statement by Entity-A); never attribute a statement to an event with no source.

### WR-ANSWER.6 — On challenge, cite, separate fact from inference, and retract overreach
**When** a user challenges a specific claim and asks for its source, the agent **shall** produce
the verbatim source quote (or state that no source exists), **shall** distinguish sourced fact
from inference, and **if** the claim is unsupported, **then** the agent **shall** retract or
correct it rather than defending the original wording.
- **Rationale:** recovery behaviour when an embellishment ships.
- **Generic example:** challenged on *"the best-in-class profile"*, the agent searches the corpus,
  finds a comparable competitor value, and retracts the superlative as unsupported inference.
- **Edge cases:** ideal end-state is that such embellishments never ship (WR-ANSWER.1/.3
  territory) — this requirement guards the *recovery* path when they do.

### WR-ANSWER.7 — Evidence gathering is scoped per-turn
**When** the gate gathers evidence for verification, the system **shall** scope evidence to the
current turn's messages, **not** the entire conversation history.
- **Rationale:** unscoped whole-history gathering grows monotonically with conversation length,
  eventually exceeding the model's hard token limit.
- **Generic example:** a fifth-turn verification checks the fifth turn's claims against the fifth
  turn's retrieved evidence, not a snowballing union of all five turns.
- **Edge cases:** a claim restated from a prior turn is carried forward by *reading* its persisted
  verdict, not by re-gathering all historical evidence (ties to WR-PROV.5).

---

## 6. Status & contradiction lifecycle

*Surface: write + lint. Defect class: a page reads as ongoing after the effort ended;
contradictory statements coexist unflagged.*

### WR-STATUS.1 — Status is explicit and dated
**When** a source establishes or changes an activity's status, the pipeline **shall** record the
status and its status date on the page.
- **Rationale:** status inferred from prose age is unreliable; make it a first-class field.
- **Generic example:** a source terminating Activity-T sets status = terminated, status date =
  2025-05.
- **Edge cases:** status inference must catch indirect phrasings ("primary objective not met",
  future-readout language) and scope negatives to the **subject** activity, not co-mentioned ones.

### WR-STATUS.2 — Status is not asserted beyond what a source supports
**If** no ingested source confirms a current status, **then** the agent **shall** state that the
status cannot be confirmed / may be stale, and **shall not** assert a definitive "ongoing/active".
- **Rationale:** a confident "ongoing" from a single old source is the loud-failure case.
- **Generic example:** Activity-T's most recent source is a year-old note with no status change →
  *"cannot confirm current status; latest source is Event-V (2024), data may be stale"*.
- **Edge cases:** must fail loudly (never fabricate a confident status) even when the status field
  is absent entirely.

### WR-STATUS.3 — Intra-source and status-vs-body contradictions are flagged
**When** lint runs, the system **shall** flag a page whose status metadata contradicts its body
(e.g. terminated status, "ongoing/future-readout" body) and whose table values contradict its
prose.
- **Rationale:** status drift and the table/prose transposition class.
- **Generic example:** a page with status = terminated but a body describing an upcoming readout →
  contradiction flag; a table cell "37.5%" attributed to a group the prose assigns elsewhere →
  table-vs-prose flag.
- **Edge cases:** guard against **false-positive** contradiction flags — a mis-fired flag that
  accuses a *correct* page is itself a defect; flags must catch the page's own contradictions, not
  those of co-mentioned entities.

### WR-STATUS.4 *(Optional)* — Contradiction flags have a durable lifecycle
**Where** a contradiction-lifecycle facility is provided, the system **shall** track each raised
contradiction flag through a lifecycle (ledger + revisit marker + SLA), including an
"unresolvable" terminal state, rather than leaving an inert inline marker.
- **Rationale:** an inline contradiction string with no lifecycle rots and misleads. This is an
  enhancement over the baseline, which surfaces contradictions only as reporting-lint entries and
  inline markers (see WR-STATUS.3); the durable ledger + SLA escalation is the optional target.
- **Generic example:** a flagged conflict either gets resolved (and the flag cleared with a reason)
  or is marked unresolvable with a note, and is surfaced by an SLA lint if it lingers.
- **Edge cases:** a resolved flag must be removed, not left as scar tissue; the ledger is the
  source of truth over scattered inline markers.

---

## 7. Query-scope discipline & synthesised artifacts

*Surface: synthesis. Defect class: mixing incomparable scopes, or laundering unsourced inference
into a durable artifact.*

### WR-SCOPE.1 — Claims are tagged with their scope and not blended across scopes
**When** the agent synthesises an answer scoped to a subset (a category, a segment, a
subpopulation), the agent **shall** tag each cited claim with its own scope and **shall not**
present a claim from a broader or different scope as evidence for the requested one without
labelling it.
- **Rationale:** the cross-scope blend — data from Scope-B folded into a Scope-A answer unlabelled.
- **Generic example:** asked about Scope-A, a data point that belongs only to Scope-B is either
  excluded or explicitly labelled "Scope-B context", never presented as Scope-A evidence.
- **Edge cases:** a self-authored persisted artifact must obey the same scope discipline — it is a
  durable vehicle for the same defect (ties to WR-KB.1).

### WR-SCOPE.2 — Comparisons use a like-for-like reference
**When** the agent benchmarks an item against a reference, the agent **shall** select a reference
in the **same category/context** and **shall** state that reference's category/context explicitly.
- **Rationale:** the "wrong benchmark" class — a real but context-mismatched reference used and
  mislabelled.
- **Generic example:** benchmarking a Scope-A item, the agent picks a Reference-X that *is* Scope-A
  and names it, not a Scope-B reference mislabelled as Scope-A.
- **Edge cases:** the gate may catch an unsupported claim, but a wrong **label** can still ship in
  a table body — the label itself is in scope.

### WR-KB.1 — Persisted artifacts are built from sources and structurally valid
**When** the agent persists a synthesised artifact (a report file or a self-authored
knowledge-base page), every quantitative claim in it **shall** trace to a cited source (or be
marked "not reported"), named entities **shall** be correct, and any knowledge-base page **shall**
be well-formed (valid metadata + captured evidence).
- **Rationale:** the gather→synthesise→persist path is where a bad claim becomes **durable corpus
  content** — a self-authored page can launder unsourced inference into the knowledge base.
- **Generic example:** a landscape review persisted as a new page carries valid metadata and
  captured evidence, names real entities, and marks unknown cells "not reported".
- **Edge cases:** the persisted page must obey scope discipline (WR-SCOPE.1) and the same
  numeric-faithfulness bar as inline answers (WR-ANSWER.1); a fabricated name in a durable page is
  worse than in an ephemeral answer.

---

## 8. Retrieval discipline & tool usage (usability)

*Surface: agent loop. Defect class: the agent spirals, abandons the right toolset, or surfaces
infrastructure errors to the user.*

### WR-USE.1 — Bounded tool-call budget with an honest stopping rule
**When** the agent answers a single well-scoped factual question, the system **shall** either reach
a result within a bounded tool-call budget or state "not reported in the available sources", and
**shall not** silently spiral by re-varying the query.
- **Rationale:** without a stopping rule, a single-fact question can spiral into unbounded
  tool calls that re-vary the query and still deliver no answer.
- **Generic example:** "What was the primary result of Activity-T?" resolves in a small number of
  targeted calls or returns an explicit not-found — a retry of the same question does not reproduce
  the spiral.
- **Edge cases:** the stopping rule may be realised as a **general per-run tool-call limiter**
  rather than a per-tool, per-turn budget — either satisfies the requirement, provided a
  low-progress / no-match stop triggers before any downstream hard usage-limit does. A per-tool
  budget is not required.

### WR-USE.2 — Knowledge-base content is retrieved via the knowledge-base tools
**While** answering a knowledge-base content question, the agent **shall** use the knowledge-base
retrieval tools (search / read / grep / outline) and **shall not** fall back to raw
shell/filesystem tools against the knowledge base's on-disk files.
- **Rationale:** falling back to raw shell/filesystem access bypasses the knowledge base's
  recency, ranking, and provenance guarantees and indicates the agent has lost track of which
  toolset applies.
- **Generic example:** answering a question about Activity-T's status uses the knowledge-base
  grep/read tools, not a raw `grep` over the corpus directory.
- **Edge cases:** verify by inspecting call *arguments*, not just tool names — a shell call
  unrelated to the knowledge base does not count as a violation.

### WR-USE.3 — Long syntheses do not surface a raw timeout mid-turn
**If** a deep multi-source synthesis or report generation would exceed the platform's request
timeout, **then** the system **shall** chunk or stream the work so no single call exceeds it, and
**shall not** surface a raw timeout error to the user.
- **Rationale:** deep multi-source synthesis is precisely the request class most likely to exceed
  a request timeout; a raw timeout mid-turn destroys work in progress.
- **Generic example:** a "thorough investigation + report" request streams/segments generation
  instead of one over-long call that trips the timeout ceiling.
- **Edge cases:** a transparent one-shot retry that recovers without a visible error is acceptable
  as a partial pass; distinguish an infrastructure timeout from the session-init race (WR-USE.4).

### WR-USE.4 — Session initialisation is transparent on the first call
**When** a fresh session issues its first knowledge-base read, the system **shall** succeed (or
transparently retry once) and **shall not** surface a "no workspace attached" error to the user.
- **Rationale:** a session whose first read fails presents an initialisation error as if it were
  an answer.
- **Generic example:** the first read of a session returns content, not an initialisation error
  framed as a result.
- **Edge cases:** distinct from an infrastructure timeout on the first model call — do not conflate
  the two.

### WR-USE.5 — Large pages are read without silent truncation
**If** a page exceeds the read cap (size or line limit), **then** the system **shall** read it via
section/outline scoping (or split it) and **shall not** silently drop content past the cut.
- **Rationale:** an oversized page truncating at the cap, silently missing everything after.
- **Generic example:** a very large page is answered via an outline plus targeted section reads, or
  the answer transparently notes which section was read.
- **Edge cases:** either a complete answer or a transparently-scoped one — never a silently partial
  one.

---

## 9. Corpus structural health

*Surface: maintenance. Defect class: duplicate page locations, dangling links, orphaned content,
force-attached irrelevant pages.*

### WR-HEALTH.1 — Scheduled structural-health lint
**When** the scheduled corpus-health check runs, the system **shall** report dangling relative
links, orphan directories, duplicate page paths, and metadata parse failures.
- **Rationale:** dangling links and duplicate-location files accumulate unnoticed at scale.
- **Generic example:** two pages for the same entity at `a/T.md` and `b/a/T.md` with divergent
  content → flagged as a duplicate-location defect.
- **Edge cases:** the check is read-only reporting; remediation (dedup/merge/redirect) is a
  separate deterministic backfill.

### WR-HEALTH.2 — Writes must not silently destroy existing content
**If** a write-path operation (section update, metadata stamp, consolidation) would delete or
overwrite existing sourced content, **then** the system **shall** preserve it (merge, not clobber)
and **shall not** silently drop it.
- **Rationale:** event sections silently destroyed by the write path; destructive "glue" content
  loss.
- **Generic example:** appending a new event section to a multi-event page must not wipe the
  earlier event sections.
- **Edge cases:** a topical-relevance guard must prevent force-attached (falsely-matched) pages
  from receiving real content in the first place.

---

## Appendix A — Requirement index (pattern × defect class)

| Req | EARS pattern | Defect class defended |
|---|---|---|
| WR-INGEST.1 | Event | evidence not captured at write |
| WR-INGEST.2 | Unwanted | fabricated number at ingest |
| WR-INGEST.3 | Event | confidence-inflation of hedged source |
| WR-INGEST.4 | Unwanted | fabricated entity identity |
| WR-INGEST.5 | Event | metadata loss on partial rewrite |
| WR-TIME.1 | Event | missing per-section data cutoff |
| WR-TIME.2 | Event | recency not surfaced on read |
| WR-TIME.3 | Event+Unwanted | stale best-match unmarked |
| WR-TIME.4 | Event | undated generated table |
| WR-TIME.5 | Event+Unwanted | old data presented as current |
| WR-RANK.1 | Event | low-authority source outranks high |
| WR-RANK.2 | Unwanted | authority overrides staleness |
| WR-RANK.3 | Optional | recent material never surfaced |
| WR-PROV.1 | Event | number without a citation |
| WR-PROV.2 | Event | citation not to the original file |
| WR-PROV.3 | Event | citation not located at the span |
| WR-PROV.4 | Event | wrong/missing origin source |
| WR-PROV.5 | Event | no claim-level provenance on result |
| WR-PROV.6 | Optional | ambiguous canonical among duplicates |
| WR-ANSWER.1 | Unwanted | unsourced number reaches user |
| WR-ANSWER.2 | Event | unknown cell invented |
| WR-ANSWER.3 | Unwanted | raw hedge tag in prose |
| WR-ANSWER.4 | Event | estimate shipped as fact |
| WR-ANSWER.5 | Unwanted | absent data fabricated |
| WR-ANSWER.6 | Event+Unwanted | overreach defended on challenge |
| WR-ANSWER.7 | Event | unbounded evidence gathering |
| WR-STATUS.1 | Event | status not explicit/dated |
| WR-STATUS.2 | Unwanted | status asserted beyond source |
| WR-STATUS.3 | Event | contradictions unflagged / false-flagged |
| WR-STATUS.4 | Optional | contradiction flag with no lifecycle |
| WR-SCOPE.1 | Event | cross-scope blending |
| WR-SCOPE.2 | Event | context-mismatched benchmark |
| WR-KB.1 | Event | unsourced inference persisted durably |
| WR-USE.1 | Event | retrieval spiral / no stopping rule |
| WR-USE.2 | State | wrong toolset for KB content |
| WR-USE.3 | Unwanted | raw timeout surfaced mid-turn |
| WR-USE.4 | Event | session-init error surfaced |
| WR-USE.5 | Unwanted | silent page truncation |
| WR-HEALTH.1 | Event | structural defects accumulate unseen |
| WR-HEALTH.2 | Unwanted | write path destroys content |

## Appendix B — Defence-in-depth map

The same defect class is defended at multiple surfaces because single-surface fixes proved
insufficient. Numeric fabrication is the clearest example:

```
FABRICATED NUMBER
  ├─ ingest surface   → WR-INGEST.2  (grounding warning at write)
  ├─ answer surface   → WR-ANSWER.1  (gate flags unsourced number)
  ├─ table cell       → WR-ANSWER.2  ("not reported", not invented)
  ├─ provenance       → WR-PROV.1    (every number carries a citation)
  └─ recovery         → WR-ANSWER.6  (retract on challenge)
```

Staleness likewise spans WR-TIME.1 (capture), WR-TIME.2/.3 (surface), WR-RANK.2 (never
outranked), WR-TIME.5 (answer states cutoff). A requirement that lives at only one surface is a
gap: prefer defending a defect class end-to-end.

## Appendix C — Known gap classes (typically requirement-incomplete)

*Appendix C covers **specification-completeness** gaps — places where the requirement itself is
hard to fully pin down. For **implementation-conformance** gaps — requirements that are complete
but not yet realised — see Appendix D.*

- **Multi-page item citations** — secondary pages of a multi-page item often resolve to the wrong
  source or none (WR-PROV.2 hard to fully satisfy for such sources).
- **Recovery vs prevention** — challenge-recovery (WR-ANSWER.6) and gather→synthesise→persist
  (WR-KB.1) behaviours are often observed-good but unguarded by tests; add regression coverage.
- **Data-dependent requirements** — capture requirements (WR-TIME.1, WR-INGEST.1) only hold for
  newly ingested pages; legacy pages gain nothing until a (usually gated) full re-ingest.
- **Recency-weighted search** (WR-RANK.3) is optional and evaluation-gated; leaving it off is a
  known ranking gap for present-but-unsurfaced recent material.

## Appendix D — Not-currently-realised requirements (conformance gaps)

The following are specified above but **not realised** by the reference implementation at time of
validation. They remain normative targets; the gap is tracked here rather than by weakening the
requirement text.

- **Contradiction lifecycle** (WR-STATUS.4, now Optional) — contradictions are detected and
  surfaced as reporting-lint entries and inline markers (WR-STATUS.3 holds), but there is no
  durable ledger, revisit marker, SLA escalation, or "unresolvable" terminal state.
- **Retrieval stopping rule** (WR-USE.1) — the per-tool, per-turn call budget was removed; a
  general per-run tool-call limiter is the intended replacement, but the honest
  low-progress/no-match stop that returns "not reported" is not guaranteed at the retrieval-tool
  layer.
- **Session-init and mid-turn timeout guards** (WR-USE.3, WR-USE.4) — no chunking/streaming of
  long syntheses nor a first-read init retry was found in the retrieval read path; these are
  infrastructure-dependent and unverified.
- **Numeric gate over written deliverables** (WR-ANSWER.1 edge case) — the gate decomposes and
  verifies the drafted answer text; whether it also diffs numbers inside code-generated files
  (reports, spreadsheets) is unconfirmed — the hardest surface and a likely gap.
