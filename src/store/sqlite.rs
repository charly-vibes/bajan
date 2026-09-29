//! Purpose: SQLite persistence of the claim graph as plain rows in a single
//! embedded database (`gm_embedded_store`) — episodes, claim nodes, lineage
//! edges, extraction-cache markers, and adoption audit records.
//! Responsibilities: durable persistence; the dump/recreate round-trip
//! (`p_embedded_store`); SQL-level adopt (`gm_human_adopt`).
//! Rationale: plain rows in one SQLite file, fully rebuildable from the
//! episode stream plus extraction output — dump-then-recreate reproduces
//! the graph exactly, so the store never becomes a second source of truth.

use crate::ingest::{EpisodeRecord, Locator};
use crate::store::{
    AuditRecord, ClaimNode, ClaimStatus, Evidence, Lineage, StoreError, dropped_hedge_markers,
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
";

/// Row mapper shared by `episodes()` and `get_episode()`: reconstructs a
/// normalized episode record from the `episodes` table (`ic_verbatim`).
fn episode_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EpisodeRecord> {
    let locator_kind: String = row.get(2)?;
    let locator: Option<String> = row.get(3)?;
    Ok(EpisodeRecord {
        id: row.get(0)?,
        text: row.get(1)?,
        locator: Locator::from_parts(&locator_kind, locator),
        source: crate::ingest::SourceMeta {
            source_type: row.get(4)?,
            data_cutoff: row.get(5)?,
            authority_tier: row.get(6)?,
            tags: serde_json::from_str(&row.get::<_, String>(7)?)
                .expect("tags column is a JSON array"),
        },
    })
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
/// plus the (episode, version) extraction-cache markers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionDump {
    pub claims: Vec<DumpedClaim>,
    pub extracted: Vec<(String, String)>,
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

    /// Every persisted episode, in stable id order.
    pub fn episodes(&self) -> Vec<EpisodeRecord> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, text, locator_kind, locator, source_type, data_cutoff,
                        authority_tier, tags FROM episodes ORDER BY id",
            )
            .expect("episodes query prepares");
        let rows = stmt
            .query_map([], episode_from_row)
            .expect("episodes query maps");
        rows.map(|r| r.expect("episode row decodes")).collect()
    }

    /// One persisted episode by stable id, or `None` when absent. Used by
    /// the ingest boundary to distinguish an unchanged re-submission from
    /// a mutated one (`ic_mutated_resubmit` vs `ic_idempotent`).
    pub fn get_episode(&self, id: &str) -> Option<EpisodeRecord> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, text, locator_kind, locator, source_type, data_cutoff,
                        authority_tier, tags FROM episodes WHERE id = ?1",
            )
            .expect("episode query prepares");
        let mut rows = stmt
            .query_map([id], episode_from_row)
            .expect("episode query maps");
        rows.next().map(|r| r.expect("episode row decodes"))
    }

    /// Persisted episode ids — the persisted-episode set for the lineage
    /// walk (`qt_lineage_traceable`).
    pub fn episode_ids(&self) -> Vec<String> {
        self.episodes().into_iter().map(|e| e.id).collect()
    }

    /// Verbatim text of a persisted episode, by stable id.
    pub fn episode_text(&self, episode_id: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT text FROM episodes WHERE id = ?1",
                params![episode_id],
                |row| row.get(0),
            )
            .optional()
            .expect("episode lookup queries")
            .flatten()
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
        let claim_key = self.claim_count() - 1;
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

    fn claim_count(&self) -> usize {
        self.conn
            .query_row("SELECT COUNT(*) FROM claims", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|n| n as usize)
            .expect("claim count queries")
    }

    /// Text of a stored claim, by key.
    pub fn claim_text(&self, claim_key: usize) -> Option<String> {
        self.conn
            .query_row(
                "SELECT text FROM claims WHERE claim_key = ?1",
                params![claim_key as i64],
                |row| row.get(0),
            )
            .optional()
            .expect("claim lookup queries")
            .flatten()
    }

    /// Lineage edges of a stored claim, by claim key (`gm_reified`).
    pub fn lineage_of(&self, claim_key: usize) -> Vec<Lineage> {
        self.claims_with_lineage()
            .into_iter()
            .find(|(key, _, _)| *key == claim_key)
            .map(|(_, _, lineage)| lineage)
            .unwrap_or_default()
    }

    /// All claims in key order with their full lineage — the raw read
    /// surface the query walk traverses.
    pub fn claims_with_lineage(&self) -> Vec<(usize, ClaimNode, Vec<Lineage>)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT claim_key, text, valid_at, invalid_at, data_cutoff, status, scope,
                        source_type, evidence_kind, evidence_text, evidence_locator
                 FROM claims ORDER BY claim_key",
            )
            .expect("claims query prepares");
        let rows = stmt
            .query_map([], |row| {
                let evidence_kind: String = row.get(8)?;
                let evidence = match evidence_kind.as_str() {
                    "span" => Evidence::Span {
                        text: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                        locator: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                    },
                    _ => Evidence::Unknown,
                };
                let status = match row.get::<_, String>(5)?.as_str() {
                    "active" => ClaimStatus::Active,
                    "rejected" => ClaimStatus::Rejected,
                    _ => ClaimStatus::Staged,
                };
                Ok((
                    row.get::<_, i64>(0)? as usize,
                    ClaimNode {
                        text: row.get(1)?,
                        valid_at: row.get(2)?,
                        invalid_at: row.get(3)?,
                        data_cutoff: row.get(4)?,
                        status,
                        scope: row.get(6)?,
                        source_type: row.get(7)?,
                        evidence,
                    },
                ))
            })
            .expect("claims query prepares");
        let mut out = Vec::new();
        for row in rows {
            let (claim_key, node) = row.expect("claim row decodes");
            out.push((claim_key, node, self.lineage_rows(claim_key)));
        }
        out
    }

    fn lineage_rows(&self, claim_key: usize) -> Vec<Lineage> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT episode_id, extractor_version FROM lineage
                 WHERE claim_key = ?1 ORDER BY rowid",
            )
            .expect("lineage query prepares");
        let rows = stmt
            .query_map(params![claim_key as i64], |row| {
                Ok(Lineage {
                    episode_id: row.get(0)?,
                    extractor_version: row.get(1)?,
                })
            })
            .expect("lineage query maps");
        rows.map(|r| r.expect("lineage row decodes")).collect()
    }

    /// Mark (episode, version) extraction as done — the per-(episode,
    /// version) cache entry. Same-version re-extraction is a no-op.
    pub fn mark_extracted(&self, episode_id: &str, extractor_version: &str) {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO extracted (episode_id, extractor_version) VALUES (?1, ?2)",
                params![episode_id, extractor_version],
            )
            .expect("cache marker writes");
    }

    /// Whether (episode, version) extraction output is already cached.
    pub fn is_extracted(&self, episode_id: &str, extractor_version: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM extracted WHERE episode_id = ?1 AND extractor_version = ?2",
                params![episode_id, extractor_version],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// Persisted episodes with no extraction output cached at this version.
    pub fn pending_episodes(&self, extractor_version: &str) -> Vec<EpisodeRecord> {
        self.episodes()
            .into_iter()
            .filter(|e| !self.is_extracted(&e.id, extractor_version))
            .collect()
    }

    /// Dump extraction output (`gm_embedded_store`): every claim with its
    /// lineage plus the extraction-cache markers.
    pub fn dump_extraction_output(&self) -> ExtractionDump {
        let claims = self
            .claims_with_lineage()
            .into_iter()
            .map(|(claim_key, node, lineage)| DumpedClaim {
                claim_key,
                node,
                lineage,
            })
            .collect();
        let extracted = self.extracted_pairs();
        ExtractionDump { claims, extracted }
    }

    fn extracted_pairs(&self) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare("SELECT episode_id, extractor_version FROM extracted ORDER BY rowid")
            .expect("extracted query prepares");
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("extracted query maps");
        rows.map(|r| r.expect("extracted row decodes")).collect()
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
            self.mark_extracted(episode_id, version);
        }
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
    pub fn audit(&self) -> Vec<AuditRecord> {
        let mut stmt = self
            .conn
            .prepare("SELECT claim_key, actor, adopted_at FROM audits ORDER BY rowid")
            .expect("audit query prepares");
        let rows = stmt
            .query_map([], |row| {
                Ok(AuditRecord {
                    claim_key: row.get::<_, i64>(0)? as usize,
                    actor: row.get(1)?,
                    adopted_at: row.get::<_, i64>(2)? as u64,
                })
            })
            .expect("audit query maps");
        rows.map(|r| r.expect("audit row decodes")).collect()
    }
}
