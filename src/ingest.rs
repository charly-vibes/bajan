//! Purpose: ingest pipeline module — persist a normalized episode stream
//! verbatim with its immutable source metadata (specs/ingestion-contract.md).
//! Responsibilities: the published episode-stream record schema
//! (`ic_stream-schema`), malformed-record rejection (`ic_malformed`),
//! NFC-normalized id identity (`ic_id_canon`), at most one persisted episode
//! per stable id (`ic_unique_id`), and idempotent re-ingest (`ic_idempotent`)
//! — one outcome record per submitted episode (`ic_outcome-schema`).
//! Rationale: bajan never parses source formats (`ic_no_format_parsing`);
//! the converter upstream does. The store persists only what the stream
//! carries — verbatim text, locator (or typed absent marker), and source
//! metadata whose cutoff is never the ingest time (`ic_date_fidelity`).

use crate::cli::BajanError;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

pub const SPEC: &str = "specs/ingestion-contract.md";

/// Structural locator for an episode (`ic_verbatim`): a structural anchor
/// when derivable, or the typed absent marker when none is — absent is a
/// distinct variant, never a string that could collide with a real locator
/// (`ic_stream-schema` absent-marker convention).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Locator {
    /// A derivable structural locator: heading anchor, text fragment,
    /// page/slide reference.
    Span(String),
    /// No locator derivable — typed absent, cannot collide with a span.
    Absent,
}

impl Locator {
    /// Tag + optional value for row persistence.
    pub fn kind(&self) -> &'static str {
        match self {
            Locator::Span(_) => "span",
            Locator::Absent => "absent",
        }
    }

    pub fn value(&self) -> Option<&str> {
        match self {
            Locator::Span(s) => Some(s),
            Locator::Absent => None,
        }
    }

    /// Rebuild from stored parts.
    pub fn from_parts(kind: &str, value: Option<String>) -> Self {
        match kind {
            "span" => Locator::Span(value.unwrap_or_default()),
            _ => Locator::Absent,
        }
    }
}

/// Immutable source metadata (`ic_verbatim`): source type, data cutoff
/// (`ic_date_fidelity`: the source's cutoff or absent — never ingest time),
/// authority tier, and optional workspace tags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMeta {
    pub source_type: String,
    pub data_cutoff: Option<String>,
    pub authority_tier: u8,
    pub tags: Vec<String>,
}

/// One normalized episode record from the converter stream
/// (`ic_stream-schema`): stable id, verbatim text, locator, and metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeRecord {
    pub id: String,
    pub text: String,
    pub locator: Locator,
    pub source: SourceMeta,
}

/// Machine-readable rejection reason (`ic_outcome_schema`): why a record
/// was refused at the ingest boundary. Stable snake_case codes. The
/// conflict/duplicate pair (`ic_mutated_resubmit`, `ic_batch_duplicate`)
/// arrived with the resubmission-policy decisions (bajan-6sw).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// No stable id: absent or whitespace-only (`ic_malformed`).
    MissingId,
    /// Verbatim text absent, empty, or whitespace-only (`ic_malformed`).
    MissingText,
    /// Required source metadata missing (`ic_malformed`).
    MissingMetadata,
    /// A stable id already persisted was re-submitted with mutated text,
    /// locator, or source metadata (`ic_mutated_resubmit`) — the persisted
    /// episode is never updated in place.
    Conflict,
    /// A later occurrence of a stable id within the same submitted stream
    /// (`ic_batch_duplicate`) — first occurrence wins, later ones are
    /// rejected per record.
    Duplicate,
}

impl RejectReason {
    /// Stable machine-readable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            RejectReason::MissingId => "missing_id",
            RejectReason::MissingText => "missing_text",
            RejectReason::MissingMetadata => "missing_metadata",
            RejectReason::Conflict => "conflict",
            RejectReason::Duplicate => "duplicate",
        }
    }
}

/// Per-episode ingest outcome (`ic_outcome-schema`): one record per
/// submitted episode — `persisted`, `already_persisted`, or `rejected`
/// with a machine-readable reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum IngestOutcome {
    /// Newly persisted (`ic_verbatim` accept path).
    Persisted,
    /// An episode with this stable id is already persisted
    /// (`ic_unique_id`/`ic_idempotent`); nothing was written.
    AlreadyPersisted,
    /// Refused — never persisted (`ic_malformed` reject path).
    Rejected { reason: RejectReason },
}

/// Persist one episode record (`ic_malformed`, `ic_id_canon`, `ic_unique_id`,
/// `ic_idempotent`): malformed records are rejected with a machine-readable
/// reason and never persisted; valid records persist verbatim; a repeated
/// stable id (compared on NFC-normalized form) reports `already_persisted`
/// and writes nothing — re-ingest never duplicates or mutates provenance
/// (`ic_idempotent`).
pub fn persist(
    db: &crate::store::sqlite::SqliteStore,
    record: &EpisodeRecord,
) -> Result<IngestOutcome, BajanError> {
    // ic_id_canon: id equality is decided on the NFC-normalized form.
    let normalized_id: String = record.id.nfc().collect();
    if normalized_id.trim().is_empty() {
        return Ok(IngestOutcome::Rejected {
            reason: RejectReason::MissingId,
        });
    }
    // ic_malformed: verbatim text must be present and non-whitespace.
    if record.text.trim().is_empty() {
        return Ok(IngestOutcome::Rejected {
            reason: RejectReason::MissingText,
        });
    }
    // ic_malformed: required source metadata must be present.
    if record.source.source_type.trim().is_empty() {
        return Ok(IngestOutcome::Rejected {
            reason: RejectReason::MissingMetadata,
        });
    }
    let canonical = EpisodeRecord {
        id: normalized_id,
        ..record.clone()
    };
    let inserted = db
        .insert_episode(&canonical)
        .map_err(|e| BajanError::Store(e.to_string()))?;
    if inserted {
        return Ok(IngestOutcome::Persisted);
    }
    // ic_mutated_resubmit vs ic_idempotent: the id already exists — compare
    // the submitted record against the persisted row verbatim. Unchanged →
    // `already_persisted`; any difference in text, locator, or source
    // metadata → rejected with the machine-readable `conflict` reason. The
    // persisted episode is never updated in place and never duplicated.
    let persisted = db
        .get_episode(&canonical.id)
        .map_err(BajanError::from)?
        .ok_or_else(|| BajanError::Store("insert ignored but episode missing".to_string()))?;
    if persisted == canonical {
        Ok(IngestOutcome::AlreadyPersisted)
    } else {
        Ok(IngestOutcome::Rejected {
            reason: RejectReason::Conflict,
        })
    }
}

/// Ingest a whole submitted stream (`ic_batch_duplicate`): per-record
/// outcomes in submission order; the first occurrence of a stable id goes
/// through the normal acceptance path and every later occurrence of that
/// id in the same stream is rejected with the machine-readable `duplicate`
/// reason — rejection is per record and never extends to other episodes
/// in the batch.
pub fn persist_stream(
    db: &crate::store::sqlite::SqliteStore,
    records: &[EpisodeRecord],
) -> Vec<Result<IngestOutcome, BajanError>> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    records
        .iter()
        .map(|record| {
            // Same NFC-normalized id as `persist`'s identity decision.
            let normalized_id: String = record.id.nfc().collect();
            if !seen.insert(normalized_id) {
                return Ok(IngestOutcome::Rejected {
                    reason: RejectReason::Duplicate,
                });
            }
            persist(db, record)
        })
        .collect()
}

/// Read back the full persisted episode stream (`gm_embedded_store`:
/// the stream half of the rebuildable pair). Read-path errors map to
/// `StoreError::Sqlite` (bajan-2sj), never a panic.
pub fn dump_stream(
    db: &crate::store::sqlite::SqliteStore,
) -> Result<Vec<EpisodeRecord>, crate::store::StoreError> {
    db.episodes()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ic_outcome_schema: reasons are machine-readable snake_case codes.
    #[test]
    fn reject_reasons_are_machine_readable() {
        assert_eq!(RejectReason::MissingId.code(), "missing_id");
        assert_eq!(RejectReason::MissingText.code(), "missing_text");
        assert_eq!(RejectReason::MissingMetadata.code(), "missing_metadata");
    }
}
