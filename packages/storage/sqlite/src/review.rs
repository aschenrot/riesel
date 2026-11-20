//! Non-replayable, exact-source-identity preview/execute handoff.
//! The reviewed operation owns its cooperative SQLite maintenance session.
use crate::{
    evolution::{EvolutionLimits, EvolutionPreview, ModelEvolution},
    execute::EvolutionReceipt,
    maintenance::SqliteMaintenance,
};
use riesel_storage::StoreError;
use std::path::Path;

/// Immutable raw source evidence, bounded by the maintenance snapshot budget.
/// These are exact persisted bytes, including unchanged models and revisions.
#[derive(PartialEq, Eq)]
pub(crate) struct SourceRow {
    pub(crate) model: String,
    pub(crate) id: Vec<u8>,
    pub(crate) revision: Vec<u8>,
    pub(crate) data: Vec<u8>,
}

/// Caller assertion of a deliberate operator decision after inspecting the
/// review summary. The future CLI must acquire this from explicit input.
#[derive(Debug, Clone, Copy)]
pub enum ReviewedEvolutionAuthorization {
    ApproveInspectedPreview,
}

/// Consuming, in-process review handle. It binds the same typed plan instance,
/// source rows, finite budgets and cooperative owner across the approval step.
/// Not serializable/replayable across invocations; abandoned review makes no
/// record changes or retained backup.
pub struct EvolutionReview<'a> {
    maintenance: SqliteMaintenance,
    plan: &'a ModelEvolution,
    limits: EvolutionLimits,
    summary: EvolutionPreview,
    original_rows: Vec<SourceRow>,
}

impl SqliteMaintenance {
    /// Preview and capture *all* source records from one consistent read
    /// snapshot, retaining exclusive cooperative ownership through approval.
    /// This does not mutate data, create a backup or authorize execution.
    pub fn review_evolution<'a>(
        self,
        plan: &'a ModelEvolution,
        limits: EvolutionLimits,
    ) -> Result<EvolutionReview<'a>, StoreError> {
        let (summary, original_rows) = self.preview_with_source_rows(plan, limits, true)?;
        Ok(EvolutionReview {
            maintenance: self,
            plan,
            limits,
            summary,
            original_rows,
        })
    }
}

impl EvolutionReview<'_> {
    /// Read-only counts and byte projections for the operator's assessment.
    pub fn preview(&self) -> EvolutionPreview {
        self.summary
    }

    pub fn database_path(&self) -> &Path {
        self.maintenance.owner.database_path()
    }

    /// Execute only after external operator approval. A verified backup must
    /// be retained; execution re-audits schema and exact preview source bytes
    /// inside the IMMEDIATE transaction. Drift fails closed before any writes.
    ///
    /// An error after SQLite COMMIT can mean the new schema is installed.
    /// The independently retained backup is never auto-restored.
    pub fn execute(
        self,
        backup_destination: impl AsRef<Path>,
        _approval: ReviewedEvolutionAuthorization,
    ) -> Result<EvolutionReceipt, StoreError> {
        self.maintenance.execute_with_expected_source(
            self.plan,
            backup_destination,
            self.limits,
            Some(&self.original_rows),
        )
    }
}
