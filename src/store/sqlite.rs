//! Purpose: SQLite persistence of the claim graph as plain rows in a single
//! embedded database (`gm_embedded_store`) — episodes, claim nodes, lineage
//! edges, extraction-cache markers, adoption audit records, supersession
//! tombstones, and re-stage records.
//! Responsibilities: durable persistence; the dump/recreate round-trip
//! (`p_embedded_store`, tombstones included); SQL-level adopt
//! (`gm_human_adopt`); SQL-level supersession and explicit re-stage
//! (`ex_supersession`); read-path error discipline (bajan-2sj) — every
//! read maps failures to `StoreError::Sqlite` carrying the governing
//! spec, so a corrupt store surfaces as an envelope, never a process
//! panic.
//! Rationale: plain rows in one SQLite file, fully rebuildable from the
//! episode stream plus extraction output — dump-then-recreate reproduces
//! the graph exactly, so the store never becomes a second source of truth.

use crate::extract::Reason;
use crate::ingest::{EpisodeRecord, Locator};
use crate::store::{
    AuditRecord, ClaimNode, ClaimStatus, EdgeLabel, EnqueueOutcome, Evidence, InvalidationProposal,
    Lineage, ReviewAuditRecord, ReviewDecision, ReviewRecord, ReviewStatus, StoreError,
    SupersessionRecord, dropped_hedge_markers,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub const SPEC: &str = "specs/graph-model.md";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS episodes (
    id TEXT PRIMARY KEY,
    text TEXT NOT NULL,
    locator_kind TEXT NOT NULL,
    locator TEXT,
    source_type TEXT NOT NULL,
    data_cutoff TEXT,
    authority_tier INTEGER NOT NULL,
    tags TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS claims (
    claim_key INTEGER PRIMARY KEY,
    text TEXT NOT NULL,
    valid_at TEXT,
    invalid_at TEXT,
    data_cutoff TEXT,
    status TEXT NOT NULL,
    scope TEXT NOT NULL,
    source_type TEXT NOT NULL,
    evidence_kind TEXT NOT NULL,
    evidence_text TEXT,
    evidence_locator TEXT
);
CREATE TABLE IF NOT EXISTS lineage (
    claim_key INTEGER NOT NULL,
    episode_id TEXT NOT NULL,
    extractor_version TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS extracted (
    episode_id TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    PRIMARY KEY (episode_id, extractor_version)
);
CREATE TABLE IF NOT EXISTS audits (
    claim_key INTEGER NOT NULL,
    actor TEXT NOT NULL,
    adopted_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS edges (
    rowid_key INTEGER PRIMARY KEY AUTOINCREMENT,
    from_claim INTEGER NOT NULL,
    label TEXT NOT NULL,
    to_claim INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS invalidations (
    rowid_key INTEGER PRIMARY KEY AUTOINCREMENT,
    claim_key INTEGER NOT NULL,
    causing_episode_id TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS supersessions (
    rowid_key INTEGER PRIMARY KEY AUTOINCREMENT,
    claim_key INTEGER NOT NULL,
    reason TEXT NOT NULL,
    superseded_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS restages (
    rowid_key INTEGER PRIMARY KEY AUTOINCREMENT,
    claim_key INTEGER NOT NULL,
    actor TEXT NOT NULL,
    restaged_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reviews (
    rowid_key INTEGER PRIMARY KEY AUTOINCREMENT,
    from_claim INTEGER NOT NULL,
    to_claim INTEGER NOT NULL,
    status TEXT NOT NULL,
    evidence TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    queued_at INTEGER NOT NULL,
    resolved_at INTEGER,
    actor TEXT,
    decision TEXT
);
CREATE TABLE IF NOT EXISTS review_audits (
    rowid_key INTEGER PRIMARY KEY AUTOINCREMENT,
    from_claim INTEGER NOT NULL,
    to_claim INTEGER NOT NULL,
    decision TEXT NOT NULL,
    actor TEXT NOT NULL,
    resolved_at INTEGER NOT NULL
);
";

/// Row mapper shared by `episodes()` and `get_episode()`: reconstructs a
/// normalized episode record from the `episodes` table (`ic_verbatim`).
/// Read-path errors — including a corrupted tags column — map to
/// `StoreError::Sqlite` carrying the governing spec (bajan-2sj), never a
/// panic: every invocation must emit the suite envelope.
/// Row mapper shared by the review reads (`er_review-schema`): reconstruct
/// a queue record from a `reviews` row — status and decision decoded
/// strictly from the published vocabularies, a corrupt row is a store
/// error, never a silent guess.
fn review_row(row: &rusqlite::Row<'_>) -> Result<ReviewRecord, StoreError> {
    let from_claim: i64 = row.get(0).map_err(sql_err)?;
    let to_claim: i64 = row.get(1).map_err(sql_err)?;
    let status_raw: String = row.get(2).map_err(sql_err)?;
    let status: ReviewStatus =
        serde_json::from_value(serde_json::Value::String(status_raw.clone())).map_err(|e| {
            StoreError::Sqlite {
                message: format!("review status decode failed ({status_raw}): {e}"),
                spec: SPEC,
            }
        })?;
    let decision_raw: Option<String> = row.get(8).map_err(sql_err)?;
    let decision = match decision_raw {
        None => None,
        Some(raw) => Some(
            serde_json::from_value::<ReviewDecision>(serde_json::Value::String(raw.clone()))
                .map_err(|e| StoreError::Sqlite {
                    message: format!("review decision decode failed ({raw}): {e}"),
                    spec: SPEC,
                })?,
        ),
    };
    Ok(ReviewRecord {
        from_claim: from_claim as usize,
        to_claim: to_claim as usize,
        status,
        evidence: row.get(3).map_err(sql_err)?,
        fingerprint: row.get(4).map_err(sql_err)?,
        queued_at: row.get::<_, i64>(5).map_err(sql_err)? as u64,
        resolved_at: row
            .get::<_, Option<i64>>(6)
            .map_err(sql_err)?
            .map(|t| t as u64),
        actor: row.get(7).map_err(sql_err)?,
        decision,
    })
}

fn episode_from_row(row: &rusqlite::Row<'_>) -> Result<EpisodeRecord, StoreError> {
    let id: String = row.get(0).map_err(sql_err)?;
    let text: String = row.get(1).map_err(sql_err)?;
    let locator_kind: String = row.get(2).map_err(sql_err)?;
    let locator: Option<String> = row.get(3).map_err(sql_err)?;
    let source_type: String = row.get(4).map_err(sql_err)?;
    let data_cutoff: Option<String> = row.get(5).map_err(sql_err)?;
    let authority_tier: u8 = row.get(6).map_err(sql_err)?;
    let tags_raw: String = row.get(7).map_err(sql_err)?;
    let tags = serde_json::from_str(&tags_raw).map_err(|e| StoreError::Sqlite {
        message: format!("tags column decode failed for episode {id}: {e}"),
        spec: SPEC,
    })?;
    Ok(EpisodeRecord {
        id,
        text,
        locator: Locator::from_parts(&locator_kind, locator),
        source: crate::ingest::SourceMeta {
            source_type,
            data_cutoff,
            authority_tier,
            tags,
        },
    })
}

/// Claim-row mapper shared by `claims_with_lineage()`: same read-path
/// error discipline as `episode_from_row` (bajan-2sj).
fn claim_from_row(row: &rusqlite::Row<'_>) -> Result<(usize, ClaimNode), StoreError> {
    let claim_key: i64 = row.get(0).map_err(sql_err)?;
    let text: String = row.get(1).map_err(sql_err)?;
    let valid_at: Option<String> = row.get(2).map_err(sql_err)?;
    let invalid_at: Option<String> = row.get(3).map_err(sql_err)?;
    let data_cutoff: Option<String> = row.get(4).map_err(sql_err)?;
    let status_raw: String = row.get(5).map_err(sql_err)?;
    let scope: String = row.get(6).map_err(sql_err)?;
    let source_type: String = row.get(7).map_err(sql_err)?;
    let evidence_kind: String = row.get(8).map_err(sql_err)?;
    let evidence = match evidence_kind.as_str() {
        "span" => Evidence::Span {
            text: row
                .get::<_, Option<String>>(9)
                .map_err(sql_err)?
                .unwrap_or_default(),
            locator: row
                .get::<_, Option<String>>(10)
                .map_err(sql_err)?
                .unwrap_or_default(),
        },
        _ => Evidence::Unknown,
    };
    let status = match status_raw.as_str() {
        "active" => ClaimStatus::Active,
        "rejected" => ClaimStatus::Rejected,
        _ => ClaimStatus::Staged,
    };
    Ok((
        claim_key as usize,
        ClaimNode {
            text,
            valid_at,
            invalid_at,
            data_cutoff,
            status,
            scope,
            source_type,
            evidence,
        },
    ))
}

/// One persisted claim with its full lineage, in dump form
/// (`p_embedded_store`): claims serialize losslessly, so dump-then-recreate
/// reproduces the graph exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DumpedClaim {
    pub claim_key: usize,
    pub node: ClaimNode,
    pub lineage: Vec<Lineage>,
}

/// The extraction-output half of the rebuildable pair: claims + lineage
/// plus the (episode, version) extraction-cache markers — and the typed
/// relation edges plus staged invalidation proposals, so a dump-recreate
/// reproduces the graph's structure exactly (`gm_embedded_store`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionDump {
    pub claims: Vec<DumpedClaim>,
    pub extracted: Vec<(String, String)>,
    /// Typed claim-relation edges (`gm_relation_typing`), row order
    /// preserved.
    #[serde(default)]
    pub edges: Vec<DumpedEdge>,
    /// Staged invalidation proposals (`ex_mutation_proposal`), row order
    /// preserved.
    #[serde(default)]
    pub invalidations: Vec<InvalidationProposal>,
    /// Supersession tombstones (`ex_supersession`), row order preserved:
    /// without these a rebuild would lose the tombstone trail behind
    /// rejected statuses (gm_embedded_store).
    #[serde(default)]
    pub supersessions: Vec<SupersessionRecord>,
    /// Entity-review queue rows (`er_review-schema`), row order preserved:
    /// without these a rebuild would lose the queue and its lifecycle
    /// states (gm_embedded_store).
    #[serde(default)]
    pub reviews: Vec<ReviewRecord>,
    /// Review-resolution audit records, row order preserved.
    #[serde(default)]
    pub review_audits: Vec<ReviewAuditRecord>,
}

/// One persisted edge in dump form: endpoints are claim keys, the label
/// a member of the published relation vocabulary (`p_relation_typing`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DumpedEdge {
    pub from_claim: usize,
    pub label: EdgeLabel,
    pub to_claim: usize,
}

/// The embedded SQLite claim-graph store: a single Connection over plain
/// rows, no hidden state.
#[derive(Debug)]
pub struct SqliteStore {
    conn: Connection,
}

fn sql_err(e: impl std::fmt::Display) -> StoreError {
    StoreError::Sqlite {
        message: e.to_string(),
        spec: SPEC,
    }
}

impl SqliteStore {
    /// Open (creating if needed) an embedded store at `path`.
    pub fn open(path: &str) -> Result<Self, StoreError> {
        let conn = Connection::open(path).map_err(sql_err)?;
        conn.execute_batch(SCHEMA).map_err(sql_err)?;
        Ok(Self { conn })
    }

    /// Open a throwaway in-memory store.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::open(":memory:")
    }

    /// Persist an episode row (`ic_verbatim`: text, locator, and metadata
    /// stored exactly as submitted). Returns `true` when a new row was
    /// created, `false` when the id already existed (`ic_unique_id`).
    pub fn insert_episode(&self, episode: &EpisodeRecord) -> Result<bool, StoreError> {
        let inserted = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO episodes
                 (id, text, locator_kind, locator, source_type, data_cutoff, authority_tier, tags)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    episode.id,
                    episode.text,
                    episode.locator.kind(),
                    episode.locator.value(),
                    episode.source.source_type,
                    episode.source.data_cutoff,
                    episode.source.authority_tier,
                    serde_json::to_string(&episode.source.tags).map_err(sql_err)?,
                ],
            )
            .map_err(sql_err)?;
        Ok(inserted > 0)
    }

    /// Every persisted episode, in stable id order. Read-path errors map
    /// to `StoreError::Sqlite` (bajan-2sj) — a corrupt store must surface
    /// as an envelope, never a panic.
    pub fn episodes(&self) -> Result<Vec<EpisodeRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, text, locator_kind, locator, source_type, data_cutoff,
                        authority_tier, tags FROM episodes ORDER BY id",
            )
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            out.push(episode_from_row(row)?);
        }
        Ok(out)
    }

    /// One persisted episode by stable id, or `None` when absent. Used by
    /// the ingest boundary to distinguish an unchanged re-submission from
    /// a mutated one (`ic_mutated_resubmit` vs `ic_idempotent`).
    pub fn get_episode(&self, id: &str) -> Result<Option<EpisodeRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, text, locator_kind, locator, source_type, data_cutoff,
                        authority_tier, tags FROM episodes WHERE id = ?1",
            )
            .map_err(sql_err)?;
        let mut rows = stmt.query([id]).map_err(sql_err)?;
        match rows.next().map_err(sql_err)? {
            Some(row) => Ok(Some(episode_from_row(row)?)),
            None => Ok(None),
        }
    }

    /// Persisted episode ids — the persisted-episode set for the lineage
    /// walk (`qt_lineage_traceable`).
    pub fn episode_ids(&self) -> Result<Vec<String>, StoreError> {
        Ok(self.episodes()?.into_iter().map(|e| e.id).collect())
    }

    /// Verbatim text of a persisted episode, by stable id.
    pub fn episode_text(&self, episode_id: &str) -> Result<Option<String>, StoreError> {
        self.conn
            .query_row(
                "SELECT text FROM episodes WHERE id = ?1",
                params![episode_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)
    }

    /// Persist a claim node with one or more lineage edges and the
    /// supporting episode text (hedge-anchor check at span persistence,
    /// `gm_hedge_anchor`). Returns the assigned claim key.
    pub fn insert_claim(
        &self,
        node: &ClaimNode,
        lineage: &[Lineage],
        supporting_episode: &str,
    ) -> Result<usize, StoreError> {
        if let Evidence::Span { text, .. } = &node.evidence {
            let dropped = dropped_hedge_markers(text, supporting_episode);
            if !dropped.is_empty() {
                return Err(StoreError::HedgeMarkerDropped {
                    markers: dropped,
                    spec: SPEC,
                });
            }
        }
        let (kind, text, locator) = match &node.evidence {
            Evidence::Span { text, locator } => {
                ("span", Some(text.as_str()), Some(locator.as_str()))
            }
            Evidence::Unknown => ("unknown", None, None),
        };
        let status = match node.status {
            crate::store::ClaimStatus::Staged => "staged",
            crate::store::ClaimStatus::Active => "active",
            crate::store::ClaimStatus::Rejected => "rejected",
        };
        self.conn
            .execute(
                "INSERT INTO claims
                 (claim_key, text, valid_at, invalid_at, data_cutoff, status, scope,
                  source_type, evidence_kind, evidence_text, evidence_locator)
                 VALUES ((SELECT COALESCE(MAX(claim_key), -1) + 1 FROM claims),
                         ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    node.text,
                    node.valid_at,
                    node.invalid_at,
                    node.data_cutoff,
                    status,
                    node.scope,
                    node.source_type,
                    kind,
                    text,
                    locator
                ],
            )
            .map_err(sql_err)?;
        let claim_key = self.claim_count()? - 1;
        self.insert_lineage(claim_key, lineage)?;
        Ok(claim_key)
    }

    /// Insert lineage edges for a claim key (`gm_reified`: one edge per
    /// supporting episode). Also used by restore with explicit keys.
    fn insert_lineage(&self, claim_key: usize, lineage: &[Lineage]) -> Result<(), StoreError> {
        for edge in lineage {
            self.conn
                .execute(
                    "INSERT INTO lineage (claim_key, episode_id, extractor_version)
                     VALUES (?1, ?2, ?3)",
                    params![claim_key as i64, edge.episode_id, edge.extractor_version],
                )
                .map_err(sql_err)?;
        }
        Ok(())
    }

    fn claim_count(&self) -> Result<usize, StoreError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM claims", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|n| n as usize)
            .map_err(sql_err)
    }

    /// Text of a stored claim, by key.
    pub fn claim_text(&self, claim_key: usize) -> Result<Option<String>, StoreError> {
        self.conn
            .query_row(
                "SELECT text FROM claims WHERE claim_key = ?1",
                params![claim_key as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)
    }

    /// Lineage edges of a stored claim, by claim key (`gm_reified`).
    pub fn lineage_of(&self, claim_key: usize) -> Result<Vec<Lineage>, StoreError> {
        Ok(self
            .claims_with_lineage()?
            .into_iter()
            .find(|(key, _, _)| *key == claim_key)
            .map(|(_, _, lineage)| lineage)
            .unwrap_or_default())
    }

    /// All claims in key order with their full lineage — the raw read
    /// surface the query walk traverses. Read-path errors map to
    /// `StoreError::Sqlite` (bajan-2sj), never a panic.
    pub fn claims_with_lineage(&self) -> Result<Vec<(usize, ClaimNode, Vec<Lineage>)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT claim_key, text, valid_at, invalid_at, data_cutoff, status, scope,
                        source_type, evidence_kind, evidence_text, evidence_locator
                 FROM claims ORDER BY claim_key",
            )
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            let (claim_key, node) = claim_from_row(row)?;
            out.push((claim_key, node, self.lineage_rows(claim_key)?));
        }
        Ok(out)
    }

    fn lineage_rows(&self, claim_key: usize) -> Result<Vec<Lineage>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT episode_id, extractor_version FROM lineage
                 WHERE claim_key = ?1 ORDER BY rowid",
            )
            .map_err(sql_err)?;
        let mut rows = stmt.query(params![claim_key as i64]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            out.push(Lineage {
                episode_id: row.get(0).map_err(sql_err)?,
                extractor_version: row.get(1).map_err(sql_err)?,
            });
        }
        Ok(out)
    }

    /// Mark (episode, version) extraction as done — the per-(episode,
    /// version) cache entry. Same-version re-extraction is a no-op.
    pub fn mark_extracted(
        &self,
        episode_id: &str,
        extractor_version: &str,
    ) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO extracted (episode_id, extractor_version) VALUES (?1, ?2)",
                params![episode_id, extractor_version],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    /// Whether (episode, version) extraction output is already cached.
    /// A query failure is a store error, never a silent cache miss.
    pub fn is_extracted(
        &self,
        episode_id: &str,
        extractor_version: &str,
    ) -> Result<bool, StoreError> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM extracted WHERE episode_id = ?1 AND extractor_version = ?2",
                params![episode_id, extractor_version],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        Ok(found.is_some())
    }

    /// Persisted episodes with no extraction output cached at this version.
    pub fn pending_episodes(
        &self,
        extractor_version: &str,
    ) -> Result<Vec<EpisodeRecord>, StoreError> {
        let mut out = Vec::new();
        for episode in self.episodes()? {
            if !self.is_extracted(&episode.id, extractor_version)? {
                out.push(episode);
            }
        }
        Ok(out)
    }

    /// Dump extraction output (`gm_embedded_store`): every claim with its
    /// lineage plus the extraction-cache markers.
    pub fn dump_extraction_output(&self) -> Result<ExtractionDump, StoreError> {
        let claims = self
            .claims_with_lineage()?
            .into_iter()
            .map(|(claim_key, node, lineage)| DumpedClaim {
                claim_key,
                node,
                lineage,
            })
            .collect();
        let extracted = self.extracted_pairs()?;
        let edges = self.edges()?;
        let invalidations = self.invalidations()?;
        let supersessions = self.supersessions()?;
        let reviews = self.reviews()?;
        let review_audits = self.review_audits()?;
        Ok(ExtractionDump {
            claims,
            extracted,
            edges: edges
                .into_iter()
                .map(|(from_claim, label, to_claim)| DumpedEdge {
                    from_claim,
                    label,
                    to_claim,
                })
                .collect(),
            invalidations,
            supersessions,
            reviews,
            review_audits,
        })
    }

    fn extracted_pairs(&self) -> Result<Vec<(String, String)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT episode_id, extractor_version FROM extracted ORDER BY rowid")
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            out.push((
                row.get::<_, String>(0).map_err(sql_err)?,
                row.get::<_, String>(1).map_err(sql_err)?,
            ));
        }
        Ok(out)
    }

    /// Rebuild store content from a dump (`p_embedded_store`): claims with
    /// lineage at their original keys, plus cache markers. Episodes must be
    /// persisted first (they are the other half of the rebuild pair).
    pub fn restore_extraction_output(&self, dump: &ExtractionDump) -> Result<(), StoreError> {
        for claim in &dump.claims {
            let status = match claim.node.status {
                ClaimStatus::Staged => "staged",
                ClaimStatus::Active => "active",
                ClaimStatus::Rejected => "rejected",
            };
            let (kind, text, locator) = match &claim.node.evidence {
                Evidence::Span { text, locator } => {
                    ("span", Some(text.as_str()), Some(locator.as_str()))
                }
                Evidence::Unknown => ("unknown", None, None),
            };
            self.conn
                .execute(
                    "INSERT INTO claims
                     (claim_key, text, valid_at, invalid_at, data_cutoff, status, scope,
                      source_type, evidence_kind, evidence_text, evidence_locator)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        claim.claim_key as i64,
                        claim.node.text,
                        claim.node.valid_at,
                        claim.node.invalid_at,
                        claim.node.data_cutoff,
                        status,
                        claim.node.scope,
                        claim.node.source_type,
                        kind,
                        text,
                        locator
                    ],
                )
                .map_err(sql_err)?;
            self.insert_lineage(claim.claim_key, &claim.lineage)?;
        }
        for (episode_id, version) in &dump.extracted {
            self.mark_extracted(episode_id, version)?;
        }
        for edge in &dump.edges {
            self.insert_edge(edge.from_claim, edge.label, edge.to_claim)?;
        }
        for proposal in &dump.invalidations {
            self.stage_invalidation_proposal(proposal.claim_key, &proposal.causing_episode_id)?;
        }
        for record in &dump.supersessions {
            self.insert_supersession_row(record.claim_key, &record.reason, record.superseded_at)?;
        }
        for record in &dump.reviews {
            self.insert_review_row(record)?;
        }
        for record in &dump.review_audits {
            self.insert_review_audit_row(record)?;
        }
        Ok(())
    }

    /// Persist a typed claim-relation edge (`gm_relation_typing`): the
    /// label is a member of the published vocabulary (the `EdgeLabel`
    /// enum makes an out-of-vocabulary label unrepresentable) and both
    /// endpoints must be persisted claims — an edge always joins two
    /// claims. Row order is preserved for dump-recreate.
    pub fn insert_edge(
        &self,
        from_claim: usize,
        label: EdgeLabel,
        to_claim: usize,
    ) -> Result<(), StoreError> {
        for endpoint in [from_claim, to_claim] {
            if !self.claim_exists(endpoint)? {
                return Err(StoreError::ClaimNotFound {
                    claim_key: endpoint,
                });
            }
        }
        let label = match label {
            EdgeLabel::Mentions => "mentions",
            EdgeLabel::Contradicts => "contradicts",
            EdgeLabel::Supports => "supports",
            EdgeLabel::DerivedFrom => "derived_from",
            EdgeLabel::PossibleDuplicateOf => "possible_duplicate_of",
        };
        self.conn
            .execute(
                "INSERT INTO edges (from_claim, label, to_claim) VALUES (?1, ?2, ?3)",
                params![from_claim as i64, label, to_claim as i64],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    /// Persisted typed edges in insertion row order (`gm_relation_typing`):
    /// endpoints as claim keys, label decoded strictly from the published
    /// vocabulary — an out-of-vocabulary row is a store error, never a
    /// silent guess.
    pub fn edges(&self) -> Result<Vec<(usize, EdgeLabel, usize)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_claim, label, to_claim FROM edges ORDER BY rowid")
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            let from_claim: i64 = row.get(0).map_err(sql_err)?;
            let label_raw: String = row.get(1).map_err(sql_err)?;
            let to_claim: i64 = row.get(2).map_err(sql_err)?;
            let label: EdgeLabel = serde_json::from_value(serde_json::Value::String(label_raw))
                .map_err(|e| StoreError::Sqlite {
                    message: format!("edge label decode failed: {e}"),
                    spec: SPEC,
                })?;
            out.push((from_claim as usize, label, to_claim as usize));
        }
        Ok(out)
    }

    /// Stage an invalidation proposal against a claim
    /// (`ex_mutation_proposal`): the proposal carries the causing-episode
    /// lineage; the claim is not mutated. Unknown claim keys are refused.
    /// Row order is preserved for dump-recreate.
    pub fn stage_invalidation_proposal(
        &self,
        claim_key: usize,
        causing_episode_id: &str,
    ) -> Result<(), StoreError> {
        if !self.claim_exists(claim_key)? {
            return Err(StoreError::ClaimNotFound { claim_key });
        }
        self.conn
            .execute(
                "INSERT INTO invalidations (claim_key, causing_episode_id) VALUES (?1, ?2)",
                params![claim_key as i64, causing_episode_id],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    /// Staged invalidation proposals in insertion row order
    /// (`ex_mutation_proposal`).
    pub fn invalidations(&self) -> Result<Vec<InvalidationProposal>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT claim_key, causing_episode_id FROM invalidations ORDER BY rowid")
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            out.push(InvalidationProposal {
                claim_key: row.get::<_, i64>(0).map_err(sql_err)? as usize,
                causing_episode_id: row.get(1).map_err(sql_err)?,
            });
        }
        Ok(out)
    }

    /// Supersede on version-bump re-extraction (`ex_supersession`), at
    /// the SQL layer — the wired mirror of the in-memory `ClaimStore`
    /// semantics: tombstones — moves to `rejected` — only this episode's
    /// `staged` claims derived from extractor versions other than the
    /// re-extract version, writing one supersession row per claim with
    /// reason `superseded_by_reextraction`. `active` and `rejected`
    /// claims (any episode, any version) and other episodes' claims are
    /// never touched; lineage and evidence stay intact. Provenance
    /// inequality is the honest prior-version test (the pipeline
    /// re-extracts in version order). Returns the superseded keys in
    /// stored row order.
    pub fn supersede_prior_versions(
        &self,
        episode_id: &str,
        extractor_version: &str,
        superseded_at: u64,
    ) -> Result<Vec<usize>, StoreError> {
        let mut superseded = Vec::new();
        for (claim_key, node, lineage) in self.claims_with_lineage()? {
            let on_episode = lineage.iter().any(|l| l.episode_id == episode_id);
            if !on_episode
                || lineage
                    .iter()
                    .any(|l| l.extractor_version == extractor_version)
            {
                continue;
            }
            if node.status == ClaimStatus::Staged {
                self.insert_supersession_row(
                    claim_key,
                    &Reason::SupersededByReextraction,
                    superseded_at,
                )?;
                self.conn
                    .execute(
                        "UPDATE claims SET status = 'rejected' WHERE claim_key = ?1",
                        params![claim_key as i64],
                    )
                    .map_err(sql_err)?;
                superseded.push(claim_key);
            }
        }
        Ok(superseded)
    }

    /// Supersession tombstone trail in insertion row order
    /// (`ex_supersession`). A corrupted reason column is a store error,
    /// never a panic (bajan-2sj).
    pub fn supersessions(&self) -> Result<Vec<SupersessionRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT claim_key, reason, superseded_at FROM supersessions ORDER BY rowid")
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            let claim_key = row.get::<_, i64>(0).map_err(sql_err)? as usize;
            let reason_raw: String = row.get(1).map_err(sql_err)?;
            let reason = serde_json::from_str(&reason_raw).map_err(|e| StoreError::Sqlite {
                message: format!(
                    "reason column decode failed for supersession of claim {claim_key}: {e}"
                ),
                spec: SPEC,
            })?;
            out.push(SupersessionRecord {
                claim_key,
                reason,
                superseded_at: row.get::<_, i64>(2).map_err(sql_err)? as u64,
            });
        }
        Ok(out)
    }

    /// Persist one supersession tombstone row (used by supersession and
    /// restore; the reason is stored as its serde encoding and decoded
    /// strictly at read).
    fn insert_supersession_row(
        &self,
        claim_key: usize,
        reason: &Reason,
        superseded_at: u64,
    ) -> Result<(), StoreError> {
        let reason_raw = serde_json::to_string(reason).map_err(|e| StoreError::Sqlite {
            message: format!("reason encode failed for claim {claim_key}: {e}"),
            spec: SPEC,
        })?;
        self.conn
            .execute(
                "INSERT INTO supersessions (claim_key, reason, superseded_at) \
                 VALUES (?1, ?2, ?3)",
                params![claim_key as i64, reason_raw, superseded_at as i64],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    /// Explicit re-stage (`ex_supersession`): the ONLY path from a
    /// superseded claim back to `staged` — a claim is eligible only when
    /// it is `rejected` AND carries a supersession tombstone. Staged,
    /// active, and gate-refused (tombstone-less rejected) claims are
    /// refused; unknown keys are refused. One restage record per action
    /// carries actor identity and timestamp.
    pub fn restage(
        &self,
        claim_key: usize,
        actor: &str,
        restaged_at: u64,
    ) -> Result<(), StoreError> {
        let current: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM claims WHERE claim_key = ?1",
                params![claim_key as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        let current = match current {
            None => return Err(StoreError::ClaimNotFound { claim_key }),
            Some(status) => match status.as_str() {
                "staged" => ClaimStatus::Staged,
                "active" => ClaimStatus::Active,
                _ => ClaimStatus::Rejected,
            },
        };
        let tombstoned: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM supersessions WHERE claim_key = ?1",
                params![claim_key as i64],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        if current != ClaimStatus::Rejected || tombstoned == 0 {
            return Err(StoreError::RestageRefused {
                claim_key,
                current,
                spec: SPEC,
            });
        }
        self.conn
            .execute(
                "UPDATE claims SET status = 'staged' WHERE claim_key = ?1",
                params![claim_key as i64],
            )
            .map_err(sql_err)?;
        self.conn
            .execute(
                "INSERT INTO restages (claim_key, actor, restaged_at) VALUES (?1, ?2, ?3)",
                params![claim_key as i64, actor, restaged_at as i64],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    fn claim_exists(&self, claim_key: usize) -> Result<bool, StoreError> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM claims WHERE claim_key = ?1",
                params![claim_key as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        Ok(found.is_some())
    }

    // ---- entity review (specs/entity-review.md, bajan-6j1) ----

    /// Queue a merge candidate for human review (`er_queue_entry`): the
    /// only way a queue row is born is this call, and the only caller is
    /// the deterministic ER pass — a candidate pair enters the queue only
    /// as a `possible_duplicate_of` edge written by that pass. Refused
    /// honestly (never silently) when the pair is already open
    /// (`AlreadyProposed`) or a prior rejection covered identical evidence
    /// (`er_repropose_guard`). Returns the outcome; row order preserved.
    pub fn enqueue_candidate(
        &self,
        from_claim: usize,
        to_claim: usize,
        evidence: &str,
        fingerprint: &str,
        queued_at: u64,
    ) -> Result<EnqueueOutcome, StoreError> {
        if from_claim == to_claim {
            return Err(StoreError::InvalidReviewPair { spec: SPEC });
        }
        for endpoint in [from_claim, to_claim] {
            if !self.claim_exists(endpoint)? {
                return Err(StoreError::ClaimNotFound {
                    claim_key: endpoint,
                });
            }
        }
        // An open proposal for the unordered pair: the queue drains, it
        // never doubles up.
        if self.open_pair_row(from_claim, to_claim)?.is_some() {
            return Ok(EnqueueOutcome::AlreadyProposed);
        }
        // Repropose guard: a rejected pair re-enters only when the
        // evidence changed since the rejection.
        if let Some(row) = self.latest_rejected_row(from_claim, to_claim)?
            && row.fingerprint == fingerprint
        {
            return Ok(EnqueueOutcome::RejectedWithoutNewEvidence);
        }
        // The queue row traces to an edge: write both.
        self.insert_edge(from_claim, EdgeLabel::PossibleDuplicateOf, to_claim)?;
        self.conn
            .execute(
                "INSERT INTO reviews (from_claim, to_claim, status, evidence, fingerprint, \
                 queued_at) VALUES (?1, ?2, 'proposed', ?3, ?4, ?5)",
                params![
                    from_claim as i64,
                    to_claim as i64,
                    evidence,
                    fingerprint,
                    queued_at as i64
                ],
            )
            .map_err(sql_err)?;
        Ok(EnqueueOutcome::Queued)
    }

    /// All review records in insertion row order (the audit trail of the
    /// queue itself: open and resolved rows alike).
    pub fn reviews(&self) -> Result<Vec<ReviewRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT from_claim, to_claim, status, evidence, fingerprint, queued_at, \
                 resolved_at, actor, decision FROM reviews ORDER BY rowid",
            )
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            out.push(review_row(row)?);
        }
        Ok(out)
    }

    /// The open queue (`er_queue_transparency`): every `proposed` row,
    /// oldest-first (queue age visible in the ordering itself) — a human
    /// can see what they are deciding and what they are leaving undecided.
    pub fn review_queue(&self) -> Result<Vec<ReviewRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT from_claim, to_claim, status, evidence, fingerprint, queued_at, \
                 resolved_at, actor, decision FROM reviews WHERE status = 'proposed' \
                 ORDER BY queued_at, rowid",
            )
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            out.push(review_row(row)?);
        }
        Ok(out)
    }

    /// Supporting-episode counts for both sides of each listed candidate
    /// (`er_queue_transparency` claim counts): how well-evidenced each
    /// side of the pair is, in the listed order.
    pub fn review_episode_counts(
        &self,
        queue: &[ReviewRecord],
    ) -> Result<Vec<(usize, usize)>, StoreError> {
        let mut out = Vec::with_capacity(queue.len());
        for record in queue {
            let count = |key: usize| -> Result<usize, StoreError> {
                self.conn
                    .query_row(
                        "SELECT COUNT(*) FROM lineage WHERE claim_key = ?1",
                        params![key as i64],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|n| n as usize)
                    .map_err(sql_err)
            };
            out.push((count(record.from_claim)?, count(record.to_claim)?));
        }
        Ok(out)
    }

    /// Resolve one open candidate (`er_human_resolution`): the ONLY path
    /// out of `proposed` — an explicit human command carrying actor
    /// identity. Approve performs the merge (`er_merge_preserves`: the
    /// union of both sides' lineage survives onto the merged entity);
    /// reject drops only the `possible_duplicate_of` edge
    /// (`er_reject_drops_edge`). Either way: the proposal edge is
    /// consumed and exactly one audit record is written.
    pub fn resolve_review(
        &self,
        from_claim: usize,
        to_claim: usize,
        decision: ReviewDecision,
        actor: &str,
        resolved_at: u64,
    ) -> Result<(), StoreError> {
        if actor.is_empty() {
            return Err(StoreError::ReviewActorRequired { spec: SPEC });
        }
        let row =
            self.open_pair_row(from_claim, to_claim)?
                .ok_or(StoreError::ReviewNotProposed {
                    from_claim,
                    to_claim,
                    spec: SPEC,
                })?;
        let new_status = match decision {
            ReviewDecision::Approved => {
                // er_merge_preserves: every lineage edge from both sides
                // survives onto the merged entity (the surviving side).
                let absorbed = self.lineage_of(to_claim)?;
                let survivor = self.lineage_of(from_claim)?;
                let missing: Vec<&Lineage> = absorbed
                    .iter()
                    .filter(|edge| {
                        !survivor.iter().any(|keep| {
                            keep.episode_id == edge.episode_id
                                && keep.extractor_version == edge.extractor_version
                        })
                    })
                    .collect();
                self.insert_lineage(
                    from_claim,
                    &missing.into_iter().cloned().collect::<Vec<Lineage>>(),
                )?;
                "resolved_approved"
            }
            ReviewDecision::Rejected => "resolved_rejected",
        };
        // The proposal edge is consumed by the resolution — after approve
        // the relation is realized, after reject it is withdrawn; the
        // queue contents keep equalling the open proposal edges.
        self.conn
            .execute(
                "DELETE FROM edges WHERE label = 'possible_duplicate_of' AND \
                 ((from_claim = ?1 AND to_claim = ?2) OR (from_claim = ?2 AND to_claim = ?1))",
                params![from_claim as i64, to_claim as i64],
            )
            .map_err(sql_err)?;
        self.conn
            .execute(
                "UPDATE reviews SET status = ?3, resolved_at = ?4, actor = ?5, decision = ?6 \
                 WHERE rowid_key = ?7",
                params![
                    from_claim as i64,
                    to_claim as i64,
                    new_status,
                    resolved_at as i64,
                    actor,
                    match decision {
                        ReviewDecision::Approved => "approved",
                        ReviewDecision::Rejected => "rejected",
                    },
                    row.0,
                ],
            )
            .map_err(sql_err)?;
        self.conn
            .execute(
                "INSERT INTO review_audits (from_claim, to_claim, decision, actor, resolved_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    from_claim as i64,
                    to_claim as i64,
                    match decision {
                        ReviewDecision::Approved => "approved",
                        ReviewDecision::Rejected => "rejected",
                    },
                    actor,
                    resolved_at as i64
                ],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    /// All review-resolution audit records in insertion row order
    /// (`er_merge_preserves` / `er_reject_drops_edge`): the receipt trail.
    pub fn review_audits(&self) -> Result<Vec<ReviewAuditRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT from_claim, to_claim, decision, actor, resolved_at \
                 FROM review_audits ORDER BY rowid",
            )
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            let from_claim: i64 = row.get(0).map_err(sql_err)?;
            let to_claim: i64 = row.get(1).map_err(sql_err)?;
            let decision_raw: String = row.get(2).map_err(sql_err)?;
            let decision: ReviewDecision = serde_json::from_value(serde_json::Value::String(
                decision_raw.clone(),
            ))
            .map_err(|e| StoreError::Sqlite {
                message: format!("review decision decode failed ({decision_raw}): {e}"),
                spec: SPEC,
            })?;
            out.push(ReviewAuditRecord {
                from_claim: from_claim as usize,
                to_claim: to_claim as usize,
                decision,
                actor: row.get(3).map_err(sql_err)?,
                resolved_at: row.get::<_, i64>(4).map_err(sql_err)? as u64,
            });
        }
        Ok(out)
    }

    /// The `proposed` row for the unordered pair, if any, with its rowid.
    fn open_pair_row(
        &self,
        from_claim: usize,
        to_claim: usize,
    ) -> Result<Option<(i64, ReviewRecord)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT from_claim, to_claim, status, evidence, fingerprint, queued_at, \
                 resolved_at, actor, decision, rowid_key FROM reviews \
                 WHERE status = 'proposed' AND \
                 ((from_claim = ?1 AND to_claim = ?2) OR (from_claim = ?2 AND to_claim = ?1))",
            )
            .map_err(sql_err)?;
        let mut rows = stmt
            .query(params![from_claim as i64, to_claim as i64])
            .map_err(sql_err)?;
        match rows.next().map_err(sql_err)? {
            Some(row) => {
                let rowid: i64 = row.get(9).map_err(sql_err)?;
                Ok(Some((rowid, review_row(row)?)))
            }
            None => Ok(None),
        }
    }

    /// The latest `resolved_rejected` row for the unordered pair, if any
    /// (`er_repropose_guard`): its fingerprint is what new evidence must
    /// differ from.
    fn latest_rejected_row(
        &self,
        from_claim: usize,
        to_claim: usize,
    ) -> Result<Option<ReviewRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT from_claim, to_claim, status, evidence, fingerprint, queued_at, \
                 resolved_at, actor, decision FROM reviews \
                 WHERE status = 'resolved_rejected' AND \
                 ((from_claim = ?1 AND to_claim = ?2) OR (from_claim = ?2 AND to_claim = ?1)) \
                 ORDER BY rowid DESC LIMIT 1",
            )
            .map_err(sql_err)?;
        let mut rows = stmt
            .query(params![from_claim as i64, to_claim as i64])
            .map_err(sql_err)?;
        match rows.next().map_err(sql_err)? {
            Some(row) => Ok(Some(review_row(row)?)),
            None => Ok(None),
        }
    }

    /// Raw review-row insert used only by dump-recreate
    /// (`gm_embedded_store`): the rebuild must reproduce queue rows and
    /// their lifecycle states verbatim — `enqueue_candidate`'s guards are
    /// pipeline semantics, not rebuild semantics.
    fn insert_review_row(&self, record: &ReviewRecord) -> Result<(), StoreError> {
        let status = match record.status {
            ReviewStatus::Proposed => "proposed",
            ReviewStatus::ResolvedApproved => "resolved_approved",
            ReviewStatus::ResolvedRejected => "resolved_rejected",
        };
        let decision = record.decision.map(|d| match d {
            ReviewDecision::Approved => "approved",
            ReviewDecision::Rejected => "rejected",
        });
        self.conn
            .execute(
                "INSERT INTO reviews (from_claim, to_claim, status, evidence, fingerprint, \
                 queued_at, resolved_at, actor, decision) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    record.from_claim as i64,
                    record.to_claim as i64,
                    status,
                    record.evidence,
                    record.fingerprint,
                    record.queued_at as i64,
                    record.resolved_at.map(|t| t as i64),
                    record.actor,
                    decision,
                ],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    /// Raw audit-row insert used only by dump-recreate.
    fn insert_review_audit_row(&self, record: &ReviewAuditRecord) -> Result<(), StoreError> {
        let decision = match record.decision {
            ReviewDecision::Approved => "approved",
            ReviewDecision::Rejected => "rejected",
        };
        self.conn
            .execute(
                "INSERT INTO review_audits (from_claim, to_claim, decision, actor, resolved_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    record.from_claim as i64,
                    record.to_claim as i64,
                    decision,
                    record.actor,
                    record.resolved_at as i64
                ],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    /// Human adopt at the SQL layer (`gm_human_adopt`): the sole path from
    /// `staged` to `active`, one audit record per claim, refused otherwise.
    pub fn adopt(&self, claim_key: usize, actor: &str, adopted_at: u64) -> Result<(), StoreError> {
        let current: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM claims WHERE claim_key = ?1",
                params![claim_key as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        match current.as_deref() {
            None => Err(StoreError::ClaimNotFound { claim_key }),
            Some(status) if status != "staged" => Err(StoreError::AdoptRefused {
                claim_key,
                current: match status {
                    "active" => ClaimStatus::Active,
                    _ => ClaimStatus::Rejected,
                },
                spec: SPEC,
            }),
            Some(_) => {
                self.conn
                    .execute(
                        "UPDATE claims SET status = 'active' WHERE claim_key = ?1",
                        params![claim_key as i64],
                    )
                    .map_err(sql_err)?;
                self.conn
                    .execute(
                        "INSERT INTO audits (claim_key, actor, adopted_at) VALUES (?1, ?2, ?3)",
                        params![claim_key as i64, actor, adopted_at as i64],
                    )
                    .map_err(sql_err)?;
                Ok(())
            }
        }
    }

    /// Audit trail (`gm_human_adopt`): one record per adopted claim.
    pub fn audit(&self) -> Result<Vec<AuditRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT claim_key, actor, adopted_at FROM audits ORDER BY rowid")
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            out.push(AuditRecord {
                claim_key: row.get::<_, i64>(0).map_err(sql_err)? as usize,
                actor: row.get(1).map_err(sql_err)?,
                adopted_at: row.get::<_, i64>(2).map_err(sql_err)? as u64,
            });
        }
        Ok(out)
    }
}
