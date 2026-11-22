//! Explicit trusted-local application build rollover for persisted V2 plans.
//!
//! This is an offline, app-owned compatibility check, NOT evidence of the OS
//! identity of a newly installed binary or an authorization grant for clients.

use super::backup::{V2SnapshotBudget, compare_exact_content};
use super::{
    FORMAT, SqlitePlanRevisionRepository, read_head, revision_to_db, storage, valid_registration,
};
use riesel_app::{
    OperationRegistry, PlanAuthoringHead, PlanAuthoringSource, PlanRevisionError,
    PlanRevisionRepository, PlanSemanticCandidate, S2DocumentLimits, S2SourceDocument,
    ValidatedExecutionPlan, inspect_s2_source,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::{collections::BTreeMap, path::Path};

impl SqlitePlanRevisionRepository {
    /// Explicitly adopt **every** persisted plan under the newly registered
    /// trusted application build, after the old application has stopped.
    ///
    /// Call on a repository opened with the NEW build's local registration
    /// stamp. The caller supplies the OLD stamp, a verified exact backup
    /// captured after quiescing the old owner, all expected heads, and the
    /// replacement build's actual registered Rust handlers.
    ///
    /// Rust-owned plans additionally require their new validated Rust source;
    /// document-owned plans are rebound against the new build's exact editor
    /// descriptors. Every plan's immutable semantic projection must match.
    /// A missing, incompatible or extra plan rejects the whole transaction.
    ///
    /// This method never launches a provider, invokes callbacks, activates a
    /// revision, or authenticates an external caller. The stamp is only local
    /// build correlation; protect the repository using app/OS ownership.
    pub fn adopt_registered_build<H: ?Sized + Send + Sync>(
        &self,
        old_registration_stamp: &str,
        expected_heads: &[PlanAuthoringHead],
        verified_backup: impl AsRef<Path>,
        budget: V2SnapshotBudget,
        new_handlers: &OperationRegistry<H>,
        new_rust_plans: &[ValidatedExecutionPlan],
        limits: S2DocumentLimits,
    ) -> Result<Vec<PlanAuthoringHead>, PlanRevisionError> {
        if !valid_registration(old_registration_stamp)
            || old_registration_stamp == self.trusted_registration_stamp
            || expected_heads.is_empty()
            || expected_heads.len() > budget.max_rows as usize
        {
            return Err(PlanRevisionError::Invalid(
                "build rollover requires distinct registrations and bounded expected heads".into(),
            ));
        }
        let verified =
            Self::verify_backup(verified_backup.as_ref(), old_registration_stamp, budget)?;
        if verified.format_version != FORMAT {
            return Err(PlanRevisionError::UnsupportedFormat(
                verified.format_version,
            ));
        }
        let backup =
            Connection::open_with_flags(verified_backup.as_ref(), OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(storage)?;
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let version: i64 = tx
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(storage)?;
        if version != FORMAT {
            return Err(PlanRevisionError::UnsupportedFormat(version));
        }
        // All writes and source changes since the verified backup must reject.
        compare_exact_content(&tx, &backup)?;
        let plan_ids = {
            let mut statement = tx
                .prepare("SELECT plan_id FROM plan_heads ORDER BY plan_id")
                .map_err(storage)?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(storage)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage)?
        };
        if plan_ids.len() != expected_heads.len() {
            return Err(PlanRevisionError::Conflict);
        }
        let mut expected = BTreeMap::new();
        for head in expected_heads {
            if head.registration_stamp != old_registration_stamp
                || expected.insert(head.plan_id.as_str(), head).is_some()
            {
                return Err(PlanRevisionError::Conflict);
            }
        }
        let mut rust_by_id = BTreeMap::new();
        for plan in new_rust_plans {
            if rust_by_id.insert(plan.description().id(), plan).is_some() {
                return Err(PlanRevisionError::Invalid(
                    "duplicate new Rust source for build rollover".into(),
                ));
            }
        }

        for plan_id in &plan_ids {
            let head = read_head(&tx, plan_id)?;
            if expected.get(plan_id.as_str()).copied() != Some(&head)
                || head.source_generation >= i64::MAX as u64
            {
                return Err(PlanRevisionError::Conflict);
            }
            let inspection: Vec<u8> = tx
                .query_row(
                    "SELECT inspection FROM semantic_revisions WHERE plan_id=?1 AND revision=?2",
                    params![plan_id, revision_to_db(head.semantic_revision)?],
                    |row| row.get(0),
                )
                .map_err(storage)?;
            let candidate = match head.source {
                PlanAuthoringSource::Rust => {
                    let plan = rust_by_id.remove(plan_id.as_str()).ok_or_else(|| {
                        PlanRevisionError::Invalid(format!(
                            "replacement build has no Rust source for plan {plan_id}"
                        ))
                    })?;
                    plan.bind(new_handlers).map_err(|error| {
                        PlanRevisionError::Invalid(format!(
                            "replacement Rust plan binding failed for {plan_id}: {error}"
                        ))
                    })?;
                    PlanSemanticCandidate::from_rust(plan)?
                }
                PlanAuthoringSource::Document => {
                    let source: Option<Option<Vec<u8>>> = tx
                        .query_row(
                            "SELECT COALESCE(sr.source_document,sh.source_document)
                         FROM plan_heads h
                         JOIN semantic_revisions sr ON sr.plan_id=h.plan_id
                             AND sr.revision=h.semantic_revision
                         LEFT JOIN source_handoffs sh ON sh.plan_id=h.plan_id
                             AND sh.generation=h.source_generation
                         WHERE h.plan_id=?1",
                            params![plan_id],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(storage)?;
                    let bytes = source.flatten().ok_or_else(|| {
                        PlanRevisionError::Storage(format!(
                            "document-owned plan {plan_id} lacks current source"
                        ))
                    })?;
                    let parsed = inspect_s2_source(&bytes, limits)?;
                    let S2SourceDocument::Editable(document) = parsed else {
                        return Err(PlanRevisionError::Invalid(format!(
                            "replacement build cannot edit or bind future source for {plan_id}"
                        )));
                    };
                    PlanSemanticCandidate::from_s2(&document, new_handlers, limits)?
                }
            };
            if candidate.snapshot().plan_id != *plan_id
                || candidate.snapshot().revision != head.semantic_revision
                || candidate.snapshot().inspection != inspection
            {
                return Err(PlanRevisionError::Invalid(format!(
                    "replacement build changes executable meaning for plan {plan_id}"
                )));
            }
        }
        if !rust_by_id.is_empty() {
            return Err(PlanRevisionError::Invalid(
                "replacement Rust sources include a plan not owned by this store".into(),
            ));
        }

        // Preserve current document bytes for the new owner generation.
        // Fencing and registration history commit in the same transaction.
        tx.execute(
            "INSERT INTO source_handoffs(
                 plan_id,generation,source_kind,semantic_anchor,source_document
             )
             SELECT h.plan_id,h.source_generation+1,h.source_kind,h.semantic_revision,
                    CASE WHEN h.source_kind='document'
                         THEN COALESCE(sr.source_document,sh.source_document)
                         ELSE NULL END
             FROM plan_heads h
             JOIN semantic_revisions sr ON sr.plan_id=h.plan_id
                 AND sr.revision=h.semantic_revision
             LEFT JOIN source_handoffs sh ON sh.plan_id=h.plan_id
                 AND sh.generation=h.source_generation",
            [],
        )
        .map_err(storage)?;
        let changed = tx
            .execute(
                "UPDATE plan_heads
             SET source_generation=source_generation+1,registration_stamp=?1
             WHERE registration_stamp=?2",
                params![self.trusted_registration_stamp, old_registration_stamp],
            )
            .map_err(storage)?;
        if changed != plan_ids.len() {
            return Err(PlanRevisionError::Conflict);
        }
        tx.execute(
            "INSERT INTO plan_migrations(
                 plan_id,generation,kind,previous_registration,next_registration,
                 semantic_anchor,source_kind
             )
             SELECT plan_id,source_generation,'registration',?1,?2,
                    semantic_revision,source_kind FROM plan_heads",
            params![old_registration_stamp, self.trusted_registration_stamp],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        plan_ids.iter().map(|id| self.head(id)).collect()
    }
}
