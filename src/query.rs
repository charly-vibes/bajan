//! Purpose: read-side query module — the first read command over the
//! persisted claim graph: a bounded, lineage-traceable search.
//! Responsibilities: honest-stopping traversal over stored rows; lineage
//! traceability filtering; read-only discipline.
//! Rationale: governed by specs/query-tools.md (`qt_bounded_traversal`,
//! `qt_lineage_traceable`, `qt_readonly`, `qt_query-schema`,
//! `qt_ranking`): every hit resolves through lineage to a persisted
//! episode, the traversal runs under a budget with a typed exhausted
//! marker and honest stopping (`complete` vs `budget-exhausted`, never a
//! silent partial answer), and reads never mutate the graph. Ranking
//! orders by the supporting episodes' source metadata reachable through
//! lineage — authority tier first, then data-cutoff recency, with
//! staleness demotion (WR-RANK.1/.2, WR-TIME.3). Scope filtering
//! (`qt_scope_filtering`) restricts a read to claims whose resolved tag
//! projection — the union of the supporting episodes' tag sets via the
//! lineage walk (`ex_tag_inheritance`) — intersects the requested scope;
//! out-of-scope claims are excluded, never silently blended (WR-SCOPE.1).
//! The contradiction query traverses `contradicts` edges plus staged
//! invalidation proposals — graph structure only, never adjudicated
//! (`qt_contradiction_query`).

use crate::store::ClaimStatus;
use crate::store::sqlite::SqliteStore;
use serde::Serialize;

/// Staleness window in days (WR-TIME.3 / EARS glossary: data cutoff older
/// than the window is stale; default 365, per-workspace config later).
pub const STALENESS_WINDOW_DAYS: i64 = 365;

/// Days since the Unix epoch for today (UTC) — the common era-integer of
/// the data-cutoff date form (ISO-8601 calendar dates), so cutoff
/// comparison and staleness arithmetic stay in one deterministic unit.
pub fn today_days_utc() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_secs() as i64;
    secs.div_euclid(86_400)
}

/// The ISO-8601 calendar date (YYYY-MM-DD) `days` after 1970-01-01 — the
/// inverse of the days-since-epoch encoding, with no timezone or locale
/// input (deterministic date arithmetic, WR-TIME.3 surfacing).
pub fn iso_from_days(days: i64) -> String {
    // Howard Hinnant's civil-from-days algorithm (public domain).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 for an ISO-8601 calendar date (YYYY-MM-DD), or
/// `None` when the text is not exactly that form (absent cutoffs stay
/// absent; malformed values are never guessed into a date).
pub fn days_from_iso(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, m, d) = (&text[0..4], &text[5..7], &text[8..10]);
    let y: i64 = y.parse().ok()?;
    let m: i64 = m.parse().ok()?;
    let d: i64 = d.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days-from-civil (Hinnant): the inverse of `iso_from_days`.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

pub const SPEC: &str = "specs/query-tools.md";

/// Typed budget status (`qt_bounded_traversal`, `qt_query-schema`
/// vocabulary): `complete` when the traversal finished within budget —
/// an empty answer then means honest not-found; `budget-exhausted` when
/// the walk stopped early with claims remaining — never a silent partial
/// answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BudgetStatus {
    Complete,
    BudgetExhausted,
}

/// One search hit (`qt_query-schema`): the claim, its stored status
/// verbatim (`qt_readonly` — nothing is dropped from results), the
/// persisted episodes its lineage resolves to (`qt_lineage_traceable`),
/// and the ranking metadata reachable through that lineage
/// (`qt_ranking`): the supporting episodes' authority tier (best =
/// lowest), the freshest data cutoff in days-since-epoch, and the
/// computed staleness flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub claim_key: usize,
    pub text: String,
    pub status: ClaimStatus,
    pub scope: String,
    pub source_type: String,
    pub data_cutoff: Option<String>,
    /// Persisted episodes reachable through the claim's lineage edges.
    pub episodes: Vec<String>,
    /// Best (lowest) authority tier across the hit's persisted episodes
    /// (`qt_ranking`: ranking metadata reachable through lineage).
    pub authority_tier: u8,
    /// Freshest (largest) data cutoff across the hit's persisted episodes,
    /// as days since 1970-01-01; `None` when no episode carries a
    /// well-formed cutoff (absent stays absent — never guessed).
    pub cutoff_days: Option<i64>,
    /// `qt_ranking` staleness flag: the hit's freshest cutoff is older
    /// than the staleness window. Absent cutoffs are never stale.
    pub stale: bool,
}

/// The staleness warning carried by the best-scoring hit when its cutoff
/// is stale (`qt_ranking` / WR-TIME.3): names the hit and the stale
/// cutoff verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StalenessWarning {
    pub claim_key: usize,
    pub data_cutoff: String,
}

/// A search answer: the honest stopping status plus the ranked hits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchResult {
    /// Typed traversal outcome (`qt_bounded_traversal`).
    pub status: BudgetStatus,
    /// The requested scope this answer was computed under
    /// (`qt_query-schema` scope field) — empty when unscoped. Echoed so
    /// a consumer can never confuse a filtered answer with an unscoped
    /// one.
    pub scope: Vec<String>,
    pub hits: Vec<Hit>,
    /// Present exactly when the best-scoring hit is stale (`qt_ranking`):
    /// a fresh low-authority answer outranking it is the expected shape,
    /// so the warning marks the demoted-best, not the ordering.
    pub staleness_warning: Option<StalenessWarning>,
}

/// The per-claim ranking key derived from lineage-reachable episode
/// metadata (`qt_ranking`): best (lowest) authority tier across the
/// claim's persisted episodes, freshest (largest) data cutoff, and the
/// staleness flag against `STALENESS_WINDOW_DAYS`. A claim with no
/// persisted backing never reaches this function (lineage-invisible).
fn ranking_metadata(
    supported_episodes: &[&crate::ingest::EpisodeRecord],
    now_days: i64,
) -> (u8, Option<i64>, bool) {
    let authority_tier = supported_episodes
        .iter()
        .map(|ep| ep.source.authority_tier)
        .min()
        .unwrap_or(u8::MAX);
    let cutoff_days = supported_episodes
        .iter()
        .filter_map(|ep| ep.source.data_cutoff.as_deref().and_then(days_from_iso))
        .max();
    let stale = cutoff_days.is_some_and(|d| d < now_days - STALENESS_WINDOW_DAYS);
    (authority_tier, cutoff_days, stale)
}

/// Search the persisted claim graph: every claim whose text contains
/// `pattern` (case-insensitive) **and** whose lineage resolves, at query
/// time, to at least one persisted episode (`qt_lineage_traceable` —
/// lineage-broken claims are invisible). Unscoped — see [`search_scoped`]
/// for the scoped form.
pub fn search(
    db: &SqliteStore,
    pattern: &str,
    budget: usize,
) -> Result<SearchResult, BajanQueryError> {
    search_inner(db, pattern, budget, None)
}

/// Scoped search (`qt_scope_filtering`, WR-SCOPE.1): restrict the answer
/// to claims whose **resolved tag projection** — the union of the
/// supporting episodes' tag sets via the lineage walk
/// (`ex_tag_inheritance`; tags are never stored on the claim) —
/// intersects the requested scope set. Out-of-scope claims are excluded
/// from the hits — never silently blended in. The predicate is pure set
/// intersection: an empty `scope` intersects no projection, so a scoped
/// answer under an empty scope is honestly empty; the unfiltered read is
/// the separate [`search`] entry point. The budget walk and
/// honest stopping are unchanged: out-of-scope claims are walked (they
/// consume budget like any non-matching claim) but never surface as
/// hits. The requested scope is echoed on the result (`qt_query-schema`
/// scope field). Reads never write (`qt_readonly`).
pub fn search_scoped(
    db: &SqliteStore,
    pattern: &str,
    budget: usize,
    scope: &[String],
) -> Result<SearchResult, BajanQueryError> {
    search_inner(db, pattern, budget, Some(scope))
}

/// The shared traversal behind both entry points: `None` scope is the
/// unfiltered read; `Some(scope)` applies `qt_scope_filtering`.
fn search_inner(
    db: &SqliteStore,
    pattern: &str,
    budget: usize,
    scope: Option<&[String]>,
) -> Result<SearchResult, BajanQueryError> {
    let episodes: Vec<crate::ingest::EpisodeRecord> =
        db.episodes().map_err(|e| BajanQueryError(e.to_string()))?;
    let persisted: Vec<String> = {
        let mut ids: Vec<String> = episodes.iter().map(|e| e.id.clone()).collect();
        ids.sort();
        ids
    };
    let needle = pattern.to_lowercase();
    let now_days = today_days_utc();
    let mut hits = Vec::new();
    let mut truncated = false;

    for (walked, (claim_key, node, lineage)) in db
        .claims_with_lineage()
        .map_err(|e| BajanQueryError(e.to_string()))?
        .into_iter()
        .enumerate()
    {
        if walked >= budget {
            truncated = true;
            break;
        }
        // qt_lineage_traceable: at least one lineage edge must resolve to
        // a persisted episode at query time.
        let supported: Vec<&crate::ingest::EpisodeRecord> = lineage
            .iter()
            .filter(|edge| persisted.binary_search(&edge.episode_id).is_ok())
            .filter_map(|edge| episodes.iter().find(|e| e.id == edge.episode_id))
            .collect();
        if supported.is_empty() {
            continue;
        }
        if node.text.to_lowercase().contains(&needle) {
            // qt_scope_filtering: the resolved tag projection is the
            // union of the supporting episodes' tags; the scope
            // predicate is pure intersection (a scoped read under an
            // empty scope intersects nothing). `None` = unfiltered.
            let in_scope = match scope {
                None => true,
                Some(scope) => supported
                    .iter()
                    .any(|ep| ep.source.tags.iter().any(|tag| scope.contains(tag))),
            };
            if !in_scope {
                continue;
            }
            let (authority_tier, cutoff_days, stale) = ranking_metadata(&supported, now_days);
            // Surface the freshest well-formed cutoff verbatim; a
            // well-formed-but-absent set stays absent.
            let data_cutoff = cutoff_days.map(iso_from_days).or_else(|| {
                supported
                    .iter()
                    .filter(|ep| ep.source.data_cutoff.is_some())
                    .for_each(|_| {});
                supported
                    .iter()
                    .find_map(|ep| ep.source.data_cutoff.clone())
            });
            let episodes: Vec<String> = supported.iter().map(|ep| ep.id.clone()).collect();
            hits.push(Hit {
                claim_key,
                text: node.text.clone(),
                status: node.status,
                scope: node.scope.clone(),
                source_type: node.source_type.clone(),
                data_cutoff,
                episodes,
                authority_tier,
                cutoff_days,
                stale,
            });
        }
    }

    // qt_ranking (WR-RANK.2): the ranking key is (authority rank if not
    // stale else demoted, recency) — a stale hit demotes below every
    // fresh hit regardless of tier; among stale hits authority still
    // orders. Then freshest cutoff (absent least fresh), claim key on
    // ties — a total, deterministic order.
    hits.sort_by(|a, b| {
        a.stale
            .cmp(&b.stale)
            .then(a.authority_tier.cmp(&b.authority_tier))
            .then(b.cutoff_days.cmp(&a.cutoff_days))
            .then(a.claim_key.cmp(&b.claim_key))
    });

    // WR-TIME.3: the best-scoring hit carries the staleness warning when
    // its cutoff is stale.
    let staleness_warning = hits
        .first()
        .filter(|hit| hit.stale)
        .map(|hit| StalenessWarning {
            claim_key: hit.claim_key,
            data_cutoff: hit.data_cutoff.clone().unwrap_or_default(),
        });

    let status = if truncated {
        BudgetStatus::BudgetExhausted
    } else {
        BudgetStatus::Complete
    };
    Ok(SearchResult {
        status,
        scope: scope.unwrap_or(&[]).to_vec(),
        hits,
        staleness_warning,
    })
}

/// Query-surface error: the search itself is deterministic, so the only
/// failure is a broken store. Kept as a distinct type so the read path
/// cannot be confused with write-path errors.
#[derive(Debug, thiserror::Error)]
#[error("query failed: {0}")]
pub struct BajanQueryError(pub String);

/// One side of a contradiction pair (`qt_contradiction_query`): the
/// claim, its stored status verbatim, and the persisted episodes its
/// lineage resolves to (`qt_lineage_traceable`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContradictionSide {
    pub claim_key: usize,
    pub text: String,
    pub status: ClaimStatus,
    /// Persisted episodes reachable through the claim's lineage edges.
    pub episodes: Vec<String>,
}

/// A pair joined by a `contradicts` edge (`qt_contradiction_query`):
/// both sides carried with their lineage and status — graph structure
/// only, never adjudicated (the query has no verdict field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContradictionPair {
    /// The claim the edge is stored from.
    pub from: ContradictionSide,
    /// The claim the edge points to.
    pub to: ContradictionSide,
}

/// A staged invalidation proposal against a queried claim
/// (`qt_contradiction_query` per `ex_mutation_proposal`): reported with
/// the claim's lineage and status — never resolved by the query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StagedInvalidation {
    pub claim_key: usize,
    pub text: String,
    pub status: ClaimStatus,
    pub episodes: Vec<String>,
    pub causing_episode_id: String,
}

/// A contradiction answer (`qt_contradiction_query`): the typed traversal
/// outcome, the contradict pairs over the queried claims, and the staged
/// invalidation proposals against them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContradictionResult {
    /// Typed traversal outcome (`qt_bounded_traversal`): both walks —
    /// edges then proposals — share one budget; `budget-exhausted` when
    /// rows remained unwalked.
    pub status: BudgetStatus,
    pub pairs: Vec<ContradictionPair>,
    pub proposals: Vec<StagedInvalidation>,
}

/// Contradiction read (`qt_contradiction_query`): over the claims in
/// `claim_keys`, return exactly the `contradicts` edges joining queried
/// claims (either direction — the edge joins the pair) plus the staged
/// invalidation proposals against them, each hit carrying both sides'
/// lineage and status. The query reports graph structure only and never
/// adjudicates which side is true. Sides whose lineage reaches no
/// persisted episode are invisible (`qt_lineage_traceable`) — a pair
/// over a lineage-broken claim is not returned. One shared budget of
/// walked rows bounds BOTH walks (`qt_bounded_traversal`, bajan-2hp):
/// edges first in stored row order, then proposals in stored row order
/// — when the budget stops either walk with rows remaining the answer
/// is `budget-exhausted`; a walk that saw every row reports `complete`
/// — and an unconnected queried claim produces no hits, honestly. Reads
/// never write (`qt_readonly`).
pub fn contradictions(
    db: &SqliteStore,
    claim_keys: &[usize],
    budget: usize,
) -> Result<ContradictionResult, BajanQueryError> {
    let episodes: Vec<crate::ingest::EpisodeRecord> =
        db.episodes().map_err(|e| BajanQueryError(e.to_string()))?;
    let persisted: Vec<String> = {
        let mut ids: Vec<String> = episodes.iter().map(|e| e.id.clone()).collect();
        ids.sort();
        ids
    };
    // Resolve each claim once: node + persisted lineage episodes.
    let mut sides: std::collections::HashMap<usize, ContradictionSide> =
        std::collections::HashMap::new();
    for (claim_key, node, lineage) in db
        .claims_with_lineage()
        .map_err(|e| BajanQueryError(e.to_string()))?
    {
        let eps: Vec<String> = lineage
            .iter()
            .filter(|edge| persisted.binary_search(&edge.episode_id).is_ok())
            .map(|edge| edge.episode_id.clone())
            .collect();
        sides.insert(
            claim_key,
            ContradictionSide {
                claim_key,
                text: node.text,
                status: node.status,
                episodes: eps,
            },
        );
    }
    let side_of = |key: usize| -> Option<ContradictionSide> {
        sides.get(&key).filter(|s| !s.episodes.is_empty()).cloned()
    };
    let queried = |key: &usize| claim_keys.contains(key);

    // qt_bounded_traversal (bajan-2hp): the budget bounds BOTH walks —
    // edges first, then proposals — under one shared counter of walked
    // rows. A proposal never walked is a proposal never reported, and
    // the status says so; a budget that covers every row is honest-
    // complete even when it exceeds the row count.
    let edges = db.edges().map_err(|e| BajanQueryError(e.to_string()))?;
    let mut pairs = Vec::new();
    let mut walked = 0usize;
    let mut truncated = false;
    for (from, label, to) in edges.iter() {
        if walked >= budget {
            truncated = true;
            break;
        }
        walked += 1;
        if *label != crate::store::EdgeLabel::Contradicts {
            continue;
        }
        if !(queried(from) || queried(to)) {
            continue;
        }
        // qt_lineage_traceable: both sides must resolve to persisted
        // episodes; a lineage-broken side hides the pair.
        let (Some(from_side), Some(to_side)) = (side_of(*from), side_of(*to)) else {
            continue;
        };
        pairs.push(ContradictionPair {
            from: from_side,
            to: to_side,
        });
    }

    let proposals = if truncated {
        // The edge walk exhausted the budget — the proposal walk never
        // ran, so nothing from it is reported.
        Vec::new()
    } else {
        let mut out = Vec::new();
        for p in db
            .invalidations()
            .map_err(|e| BajanQueryError(e.to_string()))?
        {
            if walked >= budget {
                truncated = true;
                break;
            }
            walked += 1;
            if !queried(&p.claim_key) {
                continue;
            }
            if let Some(side) = side_of(p.claim_key) {
                out.push(StagedInvalidation {
                    claim_key: p.claim_key,
                    text: side.text,
                    status: side.status,
                    episodes: side.episodes,
                    causing_episode_id: p.causing_episode_id,
                });
            }
        }
        out
    };

    let status = if truncated {
        BudgetStatus::BudgetExhausted
    } else {
        BudgetStatus::Complete
    };
    Ok(ContradictionResult {
        status,
        pairs,
        proposals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // qt_query-schema: the budget-status vocabulary is exactly the two
    // published variants — complete and budget-exhausted.
    #[test]
    fn budget_status_vocabulary_is_closed() {
        assert_eq!(
            serde_json::to_value(BudgetStatus::Complete).unwrap(),
            serde_json::json!("complete")
        );
        assert_eq!(
            serde_json::to_value(BudgetStatus::BudgetExhausted).unwrap(),
            serde_json::json!("budget-exhausted")
        );
    }
}
