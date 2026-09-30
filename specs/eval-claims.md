---
id: eval.claims
kind: intent
statement: "WHEN a claim-extraction eval run executes over a golden corpus THE bajan eval harness SHALL publish precision and recall of candidate claims against the corpus's known claim sets as a checked-in, reproducible decision artifact"
---

# harness

The extraction-eval discipline for bajan, consuming the
`ex_output_schema` extension point published by `extraction.claims` (the
claim node schema of `graph.model` is the same shape — `gm_schema_v2`).
The corpus is unambiguous on the gap this closes: every "extraction
works" claim is vendor-presented or anecdotal, no neutral
precision/recall numbers for LLM extraction exist in the report
(`knowledge-graphs-report.md` Gaps), and the report's scale guidance is
"earn the graph DB through evals" (§7.4) — measure before adding storage
complexity (§4.6). So bajan publishes its own numbers.

Discipline: golden-file corpora carry known source counts N (the
`p_reified` generator contract from `graph.model`), so recall is
computable without judging prose. Scoring is deterministic — a candidate
matches a golden claim only on content equivalence (normalized text
overlap) plus evidence-span containment into the corpus episode, never
on embedding similarity or LLM judgment. Hedge preservation is scored
explicitly: a candidate that drops a hedge marker present in its
supporting episode is a precision failure even when the content matches
(`gm_hedge_anchor` made checkable). Eval results are checked into the
repo (`evals/results.md`) as the decision artifact for storage-stack
choices — the record that decides "keep SQLite vs earn a graph DB" is a
table of numbers with the corpus, extractor version, and date that
produced them, reproducible by command. A eval run is read-only over
the store: it seeds its own corpus fixture and never mutates a real
graph — evaluation runs the same pipeline stages (ingest → extract →
gate) but into a scratch store, so evals can never contaminate adopted
knowledge (adoption stays human-only per `gm_human_adopt`).

## Constraints

| id | kind | expr | traces_to | satisfies |
|----|------|------|-----------|-----------|
| ev_golden_corpus | invariant | a golden corpus is a versioned fixture of episodes where every episode carries a known claim set — each golden claim listing its source count N (the `p_reified` contract), its evidence span within the episode text, and its hedge markers; the corpus version is stamped on every eval run record | [[eval.claims]] | |
| ev_deterministic_scoring | invariant | scoring maps a candidate claim to a golden claim only when normalized text content matches and the candidate's evidence span passes the same whitespace-collapsed containment check as `ex_evidence_containment`; identical inputs (corpus version, extractor version, seed) produce byte-identical scores — no embedding similarity and no LLM judgment anywhere in scoring | [[eval.claims]] | [[extraction.claims.ex_output_schema]] |
| ev_precision_recall | invariant | every eval run publishes precision (matched candidates / emitted candidates), recall (matched golden claims / total golden claims), and the hedge-preservation rate, computed per corpus and per extractor version, plus raw match counts so the ratios are recomputable; an eval run emits exactly one results record per (corpus version, extractor version) pair | [[eval.claims]] | |
| ev_hedge_scoring | invariant | a candidate whose evidence span or text drops a hedge marker present in the supporting golden episode within the span's coverage scores as a precision failure with reason `hedge-dropped`, distinct from a content miss, so hedge fidelity is a first-class reported number per `gm_hedge_anchor` | [[eval.claims]] | |
| ev_readonly | invariant | an eval run writes only to its own scratch store and results record — it never mutates episodes, claims, or audit records of a real store, and it never moves any claim toward `active`; the only automated state the harness can produce is the pipeline's own `staged`/`rejected` outcomes inside the scratch store, adoption remains human-only per `gm_human_adopt` | [[eval.claims]] | |
| ev_results_checked_in | invariant | eval results persist as a checked-in `evals/results.md` table — corpus version, extractor version, date, precision, recall, hedge-preservation rate, raw counts — and regenerating from the same inputs reproduces the same row values, making the storage-stack decision (keep SQLite vs earn a graph DB) auditable to its numbers | [[eval.claims]] | |
| ev_results-schema | extension_point | the eval results record schema (fields, types, requiredness, including the match-record and reason vocabulary `matched`, `content-miss`, `spurious`, `hedge-dropped`) is published for reporting specs to conform to | [[eval.claims]] | |

## Model

### States

- `ready`
- `ran`
- `published`

### Transitions

| id | from | to | guard |
|----|------|----|-------|
| execute | ready | ran | [[eval.claims.ev_golden_corpus]] |
| score | ran | published | [[eval.claims.ev_deterministic_scoring]] |

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p_golden_corpus | unit | [[eval.claims.ev_golden_corpus]] | golden corpora with episodes carrying zero to many golden claims, varied source counts N in 1..4, and mixed hedge markers | every episode in a loaded corpus exposes its full golden claim set with source counts, evidence spans, and hedge markers; a corpus without a version is rejected |
| p_deterministic_scoring | unit | [[eval.claims.ev_deterministic_scoring]] | candidate sets replayed twice against the same corpus version, containing matches, near-miss paraphrases, span-violating candidates, and duplicates | replayed runs produce identical match assignments; paraphrases without content equivalence score as misses; span-violating candidates never match; no scoring call depends on wall-clock, randomness, or network |
| p_precision_recall | unit | [[eval.claims.ev_precision_recall]] | candidate sets with known true-positive, false-positive, and false-negative counts over a seeded corpus | published precision equals matched/emitted and recall equals matched/golden exactly for the seeded counts; raw counts accompany every ratio; one results record per (corpus version, extractor version) |
| p_hedge_scoring | unit | [[eval.claims.ev_hedge_scoring]] | candidates matching golden content with dropped, preserved, and added hedge markers | hedge-dropping candidates are precision failures with reason `hedge-dropped`, never plain matches; hedge preservation rate equals preserved-hedge matches over hedge-bearing golden claims |
| p_readonly | unit | [[eval.claims.ev_readonly]] | eval runs executed against stores pre-seeded with adopted claims and audit records | after any eval run the real store's claims, statuses, and audit records are unchanged; no claim reaches `active` from an automated path; the scratch store is the only mutated store |
| p_results_checked_in | unit | [[eval.claims.ev_results_checked_in]] | repeated eval runs at fixed corpus and extractor versions, and runs at bumped versions | reruns at identical versions reproduce identical row values in `evals/results.md`; version bumps append new rows without mutating prior ones |
| p_results-schema | unit | [[eval.claims.ev_results-schema]] | results records sampled from the published schema | every published results record validates against the published schema with the closed reason vocabulary |
