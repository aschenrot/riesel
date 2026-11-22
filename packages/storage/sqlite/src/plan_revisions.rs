//! Dedicated, trusted-local SQLite implementation of the portable V2 plan
//! revision protocol. This database is intentionally separate from the
//! RecordStore provider database and has NO runtime activation side effects.
//! Connection-per-operation plus SQLite BEGIN IMMEDIATE provides cross-process
//! atomic compare-and-set; the graph runner has no SQLite dependency.

use riesel_app::{
    CheckedSourceHandoff, MAX_STORED_PLAN_BYTES, PlanAuthoringHead, PlanAuthoringSnapshot, PlanAuthoringSource,
    PlanLayoutDraft, PlanRevisionError, PlanRevisionRepository, PlanSemanticCandidate,
    StoredPlanLayout, StoredSemanticPlan, check_layout_nodes,
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const FORMAT: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE plan_heads(
    plan_id TEXT PRIMARY KEY NOT NULL,
    semantic_revision INTEGER NOT NULL CHECK(semantic_revision>0),
    source_kind TEXT NOT NULL CHECK(source_kind IN ('rust','document')),
    source_generation INTEGER NOT NULL CHECK(source_generation>0),
    registration_stamp TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE semantic_revisions(
    plan_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    inspection BLOB NOT NULL,
    node_ids BLOB NOT NULL,
    source_document BLOB,
    PRIMARY KEY(plan_id,revision),
    FOREIGN KEY(plan_id) REFERENCES plan_heads(plan_id)
) WITHOUT ROWID;
CREATE TABLE layout_heads(
    plan_id TEXT NOT NULL,
    layout_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    PRIMARY KEY(plan_id,layout_id),
    FOREIGN KEY(plan_id) REFERENCES plan_heads(plan_id)
) WITHOUT ROWID;
CREATE TABLE layout_revisions(
    plan_id TEXT NOT NULL,
    layout_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    semantic_anchor INTEGER NOT NULL CHECK(semantic_anchor>0),
    layout_json BLOB NOT NULL,
    PRIMARY KEY(plan_id,layout_id,revision),
    FOREIGN KEY(plan_id,semantic_anchor) REFERENCES semantic_revisions(plan_id,revision)
) WITHOUT ROWID;
CREATE TABLE source_handoffs(
    plan_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation>0),
    source_kind TEXT NOT NULL CHECK(source_kind IN ('rust','document')),
    semantic_anchor INTEGER NOT NULL,
    source_document BLOB,
    PRIMARY KEY(plan_id,generation),
    FOREIGN KEY(plan_id,semantic_anchor) REFERENCES semantic_revisions(plan_id,revision)
) WITHOUT ROWID;
";

fn storage(error: impl std::fmt::Display) -> PlanRevisionError {
    PlanRevisionError::Storage(error.to_string())
}

fn valid_registration(stamp: &str) -> bool {
    !stamp.is_empty()
        && stamp.len() <= 256
        && !stamp
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
}

fn revision_to_db(value: u64) -> Result<i64, PlanRevisionError> {
    i64::try_from(value)
        .map_err(|_| PlanRevisionError::Invalid("revision exceeds SQLite range".into()))
}

fn revision_from_db(value: i64) -> Result<u64, PlanRevisionError> {
    u64::try_from(value)
        .map_err(|_| PlanRevisionError::Storage("negative persisted revision".into()))
}

fn source_name(value: PlanAuthoringSource) -> &'static str {
    match value {
        PlanAuthoringSource::Rust => "rust",
        PlanAuthoringSource::Document => "document",
    }
}

fn source_from_name(value: &str) -> Result<PlanAuthoringSource, PlanRevisionError> {
    match value {
        "rust" => Ok(PlanAuthoringSource::Rust),
        "document" => Ok(PlanAuthoringSource::Document),
        _ => Err(PlanRevisionError::Storage(
            "unsupported persisted authoring source".into(),
        )),
    }
}

fn check_source(
    candidate: &PlanSemanticCandidate,
    source: PlanAuthoringSource,
) -> Result<(), PlanRevisionError> {
    if candidate.is_document_owned_candidate() == (source == PlanAuthoringSource::Document) {
        Ok(())
    } else {
        Err(PlanRevisionError::Invalid(
            "candidate source is incompatible with authoring owner".into(),
        ))
    }
}

fn read_head(connection: &Connection, id: &str) -> Result<PlanAuthoringHead, PlanRevisionError> {
    let stored: Option<(i64, String, i64, String)> = connection
        .query_row(
            "SELECT semantic_revision,source_kind,source_generation,registration_stamp
             FROM plan_heads WHERE plan_id=?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(storage)?;
    let Some((revision, source, generation, stamp)) = stored else {
        return Err(PlanRevisionError::NotFound);
    };
    Ok(PlanAuthoringHead {
        plan_id: id.to_owned(),
        semantic_revision: revision_from_db(revision)?,
        source: source_from_name(&source)?,
        source_generation: revision_from_db(generation)?,
        registration_stamp: stamp,
    })
}

/// The caller provides a trusted application **correlation** stamp at
/// construction. It is NOT cryptographic authentication of a hostile client.
/// Never export this API as a remote mutating endpoint without #75/#195 grants.
pub struct SqlitePlanRevisionRepository {
    path: PathBuf,
    trusted_registration_stamp: String,
}

impl SqlitePlanRevisionRepository {
    /// Open one dedicated V2 database. Do not point this at a RecordStore DB.
    pub fn open(
        path: impl AsRef<Path>,
        trusted_registration_stamp: impl Into<String>,
    ) -> Result<Self, PlanRevisionError> {
        let path = path.as_ref();
        let trusted_registration_stamp = trusted_registration_stamp.into();
        if path.as_os_str().is_empty()
            || path == Path::new(":memory:")
            || !valid_registration(&trusted_registration_stamp)
        {
            return Err(PlanRevisionError::Invalid(
                "a dedicated file and trusted app registration stamp are required".into(),
            ));
        }
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(storage)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(storage)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(storage)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(storage)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(storage)?;
        let journal: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(storage)?;
        let synchronous: i64 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .map_err(storage)?;
        if !journal.eq_ignore_ascii_case("wal") || synchronous != 2 {
            return Err(PlanRevisionError::Storage(
                "SQLite WAL/FULL not active".into(),
            ));
        }
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let version: i64 = tx
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(storage)?;
        match version {
            0 => {
                let tables: i64 = tx.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                    [],
                    |row|row.get(0),
                ).map_err(storage)?;
                if tables != 0 {
                    return Err(PlanRevisionError::UnsupportedFormat(0));
                }
                tx.execute_batch(SCHEMA).map_err(storage)?;
                tx.pragma_update(None, "user_version", FORMAT)
                    .map_err(storage)?;
            }
            FORMAT => {
                // Existing format is verified by requiring the declared tables.
                // No silent destructive automatic upgrade of unknown versions.
                for name in [
                    "plan_heads",
                    "semantic_revisions",
                    "layout_heads",
                    "layout_revisions",
                    "source_handoffs",
                ] {
                    let exists: i64 = tx
                        .query_row(
                            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name=?1",
                            params![name],
                            |row| row.get(0),
                        )
                        .map_err(storage)?;
                    if exists != 1 {
                        return Err(PlanRevisionError::Storage(
                            "incomplete V2 database schema".into(),
                        ));
                    }
                }
            }
            other => return Err(PlanRevisionError::UnsupportedFormat(other)),
        }
        tx.commit().map_err(storage)?;
        Ok(Self {
            path: path.to_path_buf(),
            trusted_registration_stamp,
        })
    }

    fn connection(&self) -> Result<Connection, PlanRevisionError> {
        let connection = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(storage)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(storage)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(storage)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(storage)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(storage)?;
        if version != FORMAT {
            return Err(PlanRevisionError::UnsupportedFormat(version));
        }
        Ok(connection)
    }

    fn require_local_owner(
        &self,
        persisted: &PlanAuthoringHead,
        expected: &PlanAuthoringHead,
    ) -> Result<(), PlanRevisionError> {
        if persisted.source_generation != expected.source_generation
            || persisted.source != expected.source
            || persisted.registration_stamp != expected.registration_stamp
            || persisted.registration_stamp != self.trusted_registration_stamp
        {
            return Err(PlanRevisionError::Conflict);
        }
        Ok(())
    }

    fn insert_semantic(
        transaction: &Transaction<'_>,
        candidate: &PlanSemanticCandidate,
    ) -> Result<(), PlanRevisionError> {
        let snapshot = candidate.snapshot();
        let nodes = serde_json::to_vec(&snapshot.node_ids).map_err(storage)?;
        transaction.execute(
            "INSERT INTO semantic_revisions(plan_id,revision,inspection,node_ids,source_document)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                snapshot.plan_id, revision_to_db(snapshot.revision)?,
                snapshot.inspection, nodes, candidate.document_bytes(),
            ],
        ).map_err(storage)?;
        Ok(())
    }

    /// Exact source bytes for a fenced authoring generation, if the current
    /// owner is document. Reading a historical generation is inspection only.
    pub fn source_document(
        &self,
        plan_id: &str,
        source_generation: u64,
    ) -> Result<Option<Vec<u8>>, PlanRevisionError> {
        let connection = self.connection()?;
        let stored: Option<Option<Vec<u8>>> = connection
            .query_row(
                "SELECT source_document FROM source_handoffs WHERE plan_id=?1 AND generation=?2",
                params![plan_id, revision_to_db(source_generation)?],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;
        stored.ok_or(PlanRevisionError::NotFound)
    }
}

impl PlanRevisionRepository for SqlitePlanRevisionRepository {
    fn initialize(
        &self,
        candidate: &PlanSemanticCandidate,
        source: PlanAuthoringSource,
        registration_stamp: &str,
    ) -> Result<PlanAuthoringHead, PlanRevisionError> {
        check_source(candidate, source)?;
        if registration_stamp != self.trusted_registration_stamp
            || !valid_registration(registration_stamp)
        {
            return Err(PlanRevisionError::Conflict);
        }
        let snapshot = candidate.snapshot();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        if tx
            .query_row(
                "SELECT 1 FROM plan_heads WHERE plan_id=?1",
                params![snapshot.plan_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(storage)?
            .is_some()
        {
            return Err(PlanRevisionError::Conflict);
        }
        tx.execute(
            "INSERT INTO plan_heads(plan_id,semantic_revision,source_kind,source_generation,registration_stamp)
             VALUES(?1,?2,?3,1,?4)",
            params![snapshot.plan_id,revision_to_db(snapshot.revision)?,source_name(source),registration_stamp],
        ).map_err(storage)?;
        Self::insert_semantic(&tx, candidate)?;
        tx.execute(
            "INSERT INTO source_handoffs(plan_id,generation,source_kind,semantic_anchor,source_document)
             VALUES(?1,1,?2,?3,?4)",
            params![snapshot.plan_id,source_name(source),revision_to_db(snapshot.revision)?,candidate.document_bytes()],
        ).map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.head(&snapshot.plan_id)
    }

    fn head(&self, plan_id: &str) -> Result<PlanAuthoringHead, PlanRevisionError> {
        let connection = self.connection()?;
        read_head(&connection, plan_id)
    }

    fn current_authoring(
        &self,
        plan_id: &str,
    ) -> Result<PlanAuthoringSnapshot, PlanRevisionError> {
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(storage)?;
        let head = read_head(&tx, plan_id)?;
        let document_bytes = if head.source == PlanAuthoringSource::Document {
            let revision_document: Option<Option<Vec<u8>>> = tx
                .query_row(
                    "SELECT source_document FROM semantic_revisions WHERE plan_id=?1 AND revision=?2",
                    params![plan_id, revision_to_db(head.semantic_revision)?],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage)?;
            let Some(revision_document) = revision_document else {
                return Err(PlanRevisionError::Storage(
                    "authoritative head references a missing semantic revision".into(),
                ));
            };
            let current_document = if revision_document.is_some() {
                revision_document
            } else {
                tx.query_row(
                    "SELECT source_document FROM source_handoffs WHERE plan_id=?1 AND generation=?2",
                    params![plan_id, revision_to_db(head.source_generation)?],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage)?
                .flatten()
            };
            Some(current_document.ok_or_else(|| {
                PlanRevisionError::Storage(
                    "document owner has no matching current authoring source".into(),
                )
            })?)
        } else {
            None
        };
        tx.commit().map_err(storage)?;
        Ok(PlanAuthoringSnapshot {head, document_bytes})
    }


    fn semantic(
        &self,
        plan_id: &str,
        revision: u64,
    ) -> Result<StoredSemanticPlan, PlanRevisionError> {
        let connection = self.connection()?;
        let stored: Option<(Vec<u8>,Vec<u8>)> = connection.query_row(
            "SELECT inspection,node_ids FROM semantic_revisions WHERE plan_id=?1 AND revision=?2",
            params![plan_id,revision_to_db(revision)?],
            |row|Ok((row.get(0)?,row.get(1)?)),
        ).optional().map_err(storage)?;
        let Some((inspection, nodes)) = stored else {
            return Err(PlanRevisionError::NotFound);
        };
        if inspection.len() > MAX_STORED_PLAN_BYTES {
            return Err(PlanRevisionError::Storage(
                "oversized persisted plan".into(),
            ));
        }
        let node_ids: Vec<String> = serde_json::from_slice(&nodes).map_err(storage)?;
        Ok(StoredSemanticPlan {
            plan_id: plan_id.to_owned(),
            revision,
            inspection,
            node_ids,
        })
    }

    fn semantic_document(
        &self,
        plan_id: &str,
        revision: u64,
    ) -> Result<Option<Vec<u8>>, PlanRevisionError> {
        let connection = self.connection()?;
        let stored: Option<Option<Vec<u8>>> = connection
            .query_row(
                "SELECT source_document FROM semantic_revisions WHERE plan_id=?1 AND revision=?2",
                params![plan_id, revision_to_db(revision)?],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;
        stored.ok_or(PlanRevisionError::NotFound)
    }

    fn append(
        &self,
        expected: &PlanAuthoringHead,
        next: &PlanSemanticCandidate,
    ) -> Result<PlanAuthoringHead, PlanRevisionError> {
        if next.snapshot().plan_id != expected.plan_id
            || next.snapshot().revision
                != expected.semantic_revision.checked_add(1).ok_or_else(|| {
                    PlanRevisionError::Invalid("semantic revision exhausted".into())
                })?
        {
            return Err(PlanRevisionError::Invalid(
                "new semantic revision must advance exactly once".into(),
            ));
        }
        check_source(next, expected.source)?;
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let current = read_head(&tx, &expected.plan_id)?;
        self.require_local_owner(&current, expected)?;
        if current.semantic_revision != expected.semantic_revision {
            return Err(PlanRevisionError::Conflict);
        }
        let changed = tx.execute(
            "UPDATE plan_heads SET semantic_revision=?1 WHERE plan_id=?2
             AND semantic_revision=?3 AND source_generation=?4 AND source_kind=?5 AND registration_stamp=?6",
            params![
                revision_to_db(next.snapshot().revision)?,expected.plan_id,
                revision_to_db(expected.semantic_revision)?,
                revision_to_db(expected.source_generation)?,
                source_name(expected.source),self.trusted_registration_stamp,
            ],
        ).map_err(storage)?;
        if changed != 1 {
            return Err(PlanRevisionError::Conflict);
        }
        Self::insert_semantic(&tx, next)?;
        tx.commit().map_err(storage)?;
        self.head(&expected.plan_id)
    }

    fn save_layout(
        &self,
        expected: &PlanAuthoringHead,
        expected_version: Option<u64>,
        draft: &PlanLayoutDraft,
    ) -> Result<StoredPlanLayout, PlanRevisionError> {
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let current = read_head(&tx, &expected.plan_id)?;
        self.require_local_owner(&current, expected)?;
        let nodes_bytes: Option<Vec<u8>> = tx
            .query_row(
                "SELECT node_ids FROM semantic_revisions WHERE plan_id=?1 AND revision=?2",
                params![expected.plan_id, revision_to_db(draft.semantic_anchor)?],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;
        let Some(nodes_bytes) = nodes_bytes else {
            return Err(PlanRevisionError::NotFound);
        };
        let nodes: Vec<String> = serde_json::from_slice(&nodes_bytes).map_err(storage)?;
        check_layout_nodes(draft, &nodes)?;
        let next_version = match expected_version {
            None => {
                let changed = tx.execute(
                    "INSERT OR IGNORE INTO layout_heads(plan_id,layout_id,revision) VALUES(?1,?2,1)",
                    params![expected.plan_id,draft.layout_id],
                ).map_err(storage)?;
                if changed != 1 {
                    return Err(PlanRevisionError::Conflict);
                }
                1
            }
            Some(previous) => {
                let next = previous.checked_add(1).ok_or_else(|| {
                    PlanRevisionError::Invalid("layout revision exhausted".into())
                })?;
                let changed = tx
                    .execute(
                        "UPDATE layout_heads SET revision=?1
                     WHERE plan_id=?2 AND layout_id=?3 AND revision=?4",
                        params![
                            revision_to_db(next)?,
                            expected.plan_id,
                            draft.layout_id,
                            revision_to_db(previous)?
                        ],
                    )
                    .map_err(storage)?;
                if changed != 1 {
                    return Err(PlanRevisionError::Conflict);
                }
                next
            }
        };
        let bytes = serde_json::to_vec(draft).map_err(storage)?;
        tx.execute(
            "INSERT INTO layout_revisions(plan_id,layout_id,revision,semantic_anchor,layout_json)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                expected.plan_id,
                draft.layout_id,
                revision_to_db(next_version)?,
                revision_to_db(draft.semantic_anchor)?,
                bytes,
            ],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(StoredPlanLayout {
            plan_id: expected.plan_id.clone(),
            version: next_version,
            layout: draft.clone(),
        })
    }

    fn layout(
        &self,
        plan_id: &str,
        layout_id: &str,
        version: u64,
    ) -> Result<StoredPlanLayout, PlanRevisionError> {
        let connection = self.connection()?;
        let bytes: Option<Vec<u8>> = connection.query_row(
            "SELECT layout_json FROM layout_revisions WHERE plan_id=?1 AND layout_id=?2 AND revision=?3",
            params![plan_id,layout_id,revision_to_db(version)?],
            |row|row.get(0),
        ).optional().map_err(storage)?;
        let bytes = bytes.ok_or(PlanRevisionError::NotFound)?;
        let layout: PlanLayoutDraft = serde_json::from_slice(&bytes).map_err(storage)?;
        if layout.layout_id != layout_id {
            return Err(PlanRevisionError::Storage(
                "corrupted layout identity".into(),
            ));
        }
        Ok(StoredPlanLayout {
            plan_id: plan_id.to_owned(),
            version,
            layout,
        })
    }

    fn handoff(
        &self,
        expected: &PlanAuthoringHead,
        proof: &CheckedSourceHandoff,
    ) -> Result<PlanAuthoringHead, PlanRevisionError> {
        if expected.plan_id != proof.plan_id()
            || expected.semantic_revision != proof.revision()
            || expected.source == proof.target()
        {
            return Err(PlanRevisionError::Invalid(
                "source transfer does not match current plan and target".into(),
            ));
        }
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let current = read_head(&tx, &expected.plan_id)?;
        self.require_local_owner(&current, expected)?;
        if current.semantic_revision != expected.semantic_revision {
            return Err(PlanRevisionError::Conflict);
        }
        let projection: Vec<u8> = tx
            .query_row(
                "SELECT inspection FROM semantic_revisions WHERE plan_id=?1 AND revision=?2",
                params![
                    expected.plan_id,
                    revision_to_db(expected.semantic_revision)?
                ],
                |row| row.get(0),
            )
            .map_err(storage)?;
        if projection != proof.inspection() {
            return Err(PlanRevisionError::Conflict);
        }
        let next_generation = current
            .source_generation
            .checked_add(1)
            .ok_or_else(|| PlanRevisionError::Invalid("source generation exhausted".into()))?;
        let changed=tx.execute(
            "UPDATE plan_heads SET source_kind=?1,source_generation=?2 WHERE plan_id=?3
             AND semantic_revision=?4 AND source_kind=?5 AND source_generation=?6 AND registration_stamp=?7",
            params![
                source_name(proof.target()),revision_to_db(next_generation)?,expected.plan_id,
                revision_to_db(expected.semantic_revision)?,source_name(expected.source),
                revision_to_db(expected.source_generation)?,self.trusted_registration_stamp,
            ],
        ).map_err(storage)?;
        if changed != 1 {
            return Err(PlanRevisionError::Conflict);
        }
        tx.execute(
            "INSERT INTO source_handoffs(plan_id,generation,source_kind,semantic_anchor,source_document)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                expected.plan_id,revision_to_db(next_generation)?,source_name(proof.target()),
                revision_to_db(expected.semantic_revision)?,proof.document_bytes(),
            ],
        ).map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.head(&expected.plan_id)
    }
}
