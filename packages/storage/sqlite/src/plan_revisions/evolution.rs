//! Explicit, backup-backed physical V2 format migration. No implicit
//! migration on open, no handler reinterpretation, no application activation.
//! Requires the trusted application owner to quiesce live use before adopting
//! the migrated store; SQLite transactional CAS fences stale local writers.

use super::backup::{V2SnapshotBudget, compare_exact_content};
use super::{FORMAT, LEGACY_FORMAT, SqlitePlanRevisionRepository, read_head, storage};
use riesel_app::{PlanAuthoringHead, PlanRevisionError, PlanRevisionRepository};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use std::path::Path;

const MIGRATION_TABLE: &str = "
CREATE TABLE plan_migrations(
    plan_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation>0),
    kind TEXT NOT NULL CHECK(kind IN ('format','registration')),
    previous_registration TEXT NOT NULL,
    next_registration TEXT NOT NULL,
    semantic_anchor INTEGER NOT NULL CHECK(semantic_anchor>0),
    source_kind TEXT NOT NULL CHECK(source_kind IN ('rust','document')),
    PRIMARY KEY(plan_id,generation),
    FOREIGN KEY(plan_id,semantic_anchor) REFERENCES semantic_revisions(plan_id,revision)
) WITHOUT ROWID;
";

impl SqlitePlanRevisionRepository {
    /// Explicit owner-controlled legacy format 1 -> format 2 upgrade.
    ///
    /// Requires an independently **verified matching v1 snapshot** created
    /// before this call. No live application session is activated or stopped
    /// by this SDK function; caller must first quiesce its app-owned process.
    /// An existing new-format database is never downgraded or re-migrated.
    ///
    /// Every plan source generation is fenced in the SAME SQLite transaction
    /// that creates the new evolution ledger and advances user_version.
    pub fn migrate_v1_to_v2(
        &self,
        expected: &PlanAuthoringHead,
        backup_path: impl AsRef<Path>,
        budget: V2SnapshotBudget,
    ) -> Result<PlanAuthoringHead, PlanRevisionError> {
        let backup_path = backup_path.as_ref();
        let verified = Self::verify_backup(backup_path, &self.trusted_registration_stamp, budget)?;
        if verified.format_version != LEGACY_FORMAT {
            return Err(PlanRevisionError::UnsupportedFormat(
                verified.format_version,
            ));
        }
        let backup = Connection::open_with_flags(backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(storage)?;
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let version: i64 = tx
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(storage)?;
        if version != LEGACY_FORMAT {
            return Err(PlanRevisionError::UnsupportedFormat(version));
        }
        let current = read_head(&tx, &expected.plan_id)?;
        if current != *expected || current.registration_stamp != self.trusted_registration_stamp {
            return Err(PlanRevisionError::Conflict);
        }
        // The supplied independent copy must describe the EXACT current
        // database state. Concurrent modifications since backup reject, not
        // migrate a stale set of heads/source generations.
        compare_exact_content(&tx, &backup)?;
        let other_registration: i64 = tx
            .query_row(
                "SELECT count(*) FROM plan_heads WHERE registration_stamp <> ?1",
                params![self.trusted_registration_stamp],
                |row| row.get(0),
            )
            .map_err(storage)?;
        if other_registration != 0 {
            return Err(PlanRevisionError::Conflict);
        }
        let exhausted = tx
            .query_row(
                "SELECT count(*) FROM plan_heads WHERE source_generation >= 9223372036854775807",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(storage)?;
        if exhausted != 0 {
            return Err(PlanRevisionError::Invalid(
                "source generation exhausted".into(),
            ));
        }
        // Each document-owned head must retain its exact current S2 source
        // for the new generation, not the older handoff's document when an
        // intervening semantic revision has been published.
        let orphan_document: i64 = tx
            .query_row(
                "SELECT count(*) FROM plan_heads h
                 JOIN semantic_revisions sr ON sr.plan_id=h.plan_id
                    AND sr.revision=h.semantic_revision
                 LEFT JOIN source_handoffs sh ON sh.plan_id=h.plan_id
                    AND sh.generation=h.source_generation
                 WHERE h.source_kind='document'
                   AND COALESCE(sr.source_document, sh.source_document) IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(storage)?;
        if orphan_document != 0 {
            return Err(PlanRevisionError::Storage(
                "document source missing during format migration".into(),
            ));
        }
        tx.execute_batch(MIGRATION_TABLE).map_err(storage)?;
        tx.execute(
            "INSERT INTO source_handoffs(
                 plan_id,generation,source_kind,semantic_anchor,source_document
             )
             SELECT h.plan_id, h.source_generation + 1, h.source_kind,
                    h.semantic_revision,
                    CASE WHEN h.source_kind='document'
                         THEN COALESCE(sr.source_document, sh.source_document)
                         ELSE NULL END
             FROM plan_heads h
             JOIN semantic_revisions sr ON sr.plan_id=h.plan_id
                AND sr.revision=h.semantic_revision
             LEFT JOIN source_handoffs sh ON sh.plan_id=h.plan_id
                AND sh.generation=h.source_generation",
            [],
        )
        .map_err(storage)?;
        tx.execute(
            "UPDATE plan_heads SET source_generation=source_generation+1",
            [],
        )
        .map_err(storage)?;
        tx.execute(
            "INSERT INTO plan_migrations(
                 plan_id,generation,kind,previous_registration,next_registration,
                 semantic_anchor,source_kind
             )
             SELECT h.plan_id,h.source_generation,'format',
                    h.registration_stamp,h.registration_stamp,
                    h.semantic_revision,h.source_kind
             FROM plan_heads h",
            [],
        )
        .map_err(storage)?;
        tx.pragma_update(None, "user_version", FORMAT)
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.head(&expected.plan_id)
    }
}
