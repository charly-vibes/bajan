# Purpose: Records the disposition of every hand-maintained document from bajan's bootstrap inventory against the kaya-generated record projections (docs/research/, docs/public/).
# Responsibilities:
# - One row per bootstrap-inventory item with a final disposition: retired (git rm) or kept, with the reason.
# - Serves as the ticket-adjacent record for the bajan-s7e.7 SSOT cutover; docs/research/ and docs/public/ are generated and must not hold hand-edited disposition tables.
# Rationale: bajan-s7e.7 — the kaya record (.kaya/ledger.jsonl) is the single
#   source of truth; hand-maintained docs may be retired only when they
#   duplicate a generated projection AND nothing links to them.

## Dispositions

| Item | Disposition | Reason |
|---|---|---|
| `docs/burnin-deepseek-v4-flash-profit-first.org` | Kept | Not a duplicate: the generated projections do not reproduce its content — docs/research/index.md and docs/research/reproduction-package.md cite it as an ingested ledger Source (`bajan/Source/docs-burnin-...@64792fe`), and README.md (§ Burn-in findings) plus evals/results.md link to it. It is raw burn-in evidence the record indexes, not a projection of it. |
| `docs/knowledge-assistant-ears-spec.md` | Kept | Not a duplicate: the generated projections reference it only as an ingested Source. It is the behavioral acceptance oracle cited by specs/ and openspec/specs/ (graph-model, ingestion-contract, query-tools) and openspec/project.md; retiring it would break those references and the record's source graph. |
| `knowledge-graphs-report.md` | Kept | Not a duplicate: cited throughout specs/ and openspec/ (openspec/project.md calls it "the authoritative domain synthesis") and linked from README.md; the generated projections ingest it as a Source (`bajan/Source/knowledge-graphs-report.md@64792fe`) rather than reproducing it. |
| `README.md` | Kept | Repository entry point; no generated projection covers it, and it links the record workflow (§ Research record, added in bajan-s7e.7). |

## Result

Zero retirements: every bootstrap-inventory item is either the entry point or
a source artifact the ledger ingests and other specs link to — none
duplicates a generated projection. Retirement would break kaya's source
graph (blob-pinned `bajan/Source/...` references) and inbound spec links.
