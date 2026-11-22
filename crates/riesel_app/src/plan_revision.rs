//! Portable, trusted-local V2 semantic/layout revision-store contract.
//!
//! This module defines no database, remote client, identity credential,
//! application publisher, graph activation, or second execution engine.
//! Implementations must atomically enforce every documented predecessor
//! and source-generation condition against their authoritative durable state.

use crate::{
    OperationRegistry, PlanSourceCoverageReport, S2DocumentError, S2DocumentLimits, S2PlanDocument,
    S2SourceDocument, ValidatedExecutionPlan, bind_s2_document,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Limit the initial local persistence profile to bounded plans and layouts.
pub const MAX_STORED_PLAN_BYTES: usize = 1024 * 1024;
pub const MAX_STORED_LAYOUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanAuthoringSource {
    Rust,
    Document,
}

/// Snapshot returned by the *authoritative store*, not an authentication
/// token. It is a compare-and-set precursor for trusted app-owned callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanAuthoringHead {
    pub plan_id: String,
    pub semantic_revision: u64,
    pub source: PlanAuthoringSource,
    pub source_generation: u64,
    /// Installation/build correlation only. A copied string is NOT a principal.
    pub registration_stamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSemanticPlan {
    pub plan_id: String,
    pub revision: u64,
    pub inspection: Vec<u8>,
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSemanticCandidate {
    snapshot: StoredSemanticPlan,
    document_bytes: Option<Vec<u8>>,
}

impl PlanSemanticCandidate {
    /// Preserve the real validated Rust plan's immutable inspection meaning;
    /// this projection is not an executable import document.
    pub fn from_rust(plan: &ValidatedExecutionPlan) -> Result<Self, PlanRevisionError> {
        Self::new(plan, None)
    }

    /// Require a real exact-version registered application descriptor binder.
    /// A document, by itself, can never grant binding or publication rights.
    pub fn from_s2<H: ?Sized + Send + Sync>(
        document: &S2PlanDocument,
        handlers: &OperationRegistry<H>,
        limits: S2DocumentLimits,
    ) -> Result<Self, PlanRevisionError> {
        let bound = bind_s2_document(document, handlers, limits)?;
        let bytes =
            S2SourceDocument::Editable(document.clone()).serialize_checked(handlers, limits)?;
        Self::new(bound.plan(), Some(bytes))
    }

    fn new(
        plan: &ValidatedExecutionPlan,
        document_bytes: Option<Vec<u8>>,
    ) -> Result<Self, PlanRevisionError> {
        let description = plan.description();
        let inspection = serde_json::to_vec(description)
            .map_err(|error| PlanRevisionError::Invalid(error.to_string()))?;
        let node_ids: Vec<String> = description
            .operations()
            .iter()
            .map(|node| node.id().to_owned())
            .collect();
        if inspection.len() > MAX_STORED_PLAN_BYTES
            || document_bytes
                .as_ref()
                .is_some_and(|bytes| bytes.len() > MAX_STORED_PLAN_BYTES)
            || node_ids.len() > 4096
        {
            return Err(PlanRevisionError::Invalid(
                "plan exceeds persistence bounds".into(),
            ));
        }
        Ok(Self {
            snapshot: StoredSemanticPlan {
                plan_id: description.id().to_owned(),
                revision: description.revision(),
                inspection,
                node_ids,
            },
            document_bytes,
        })
    }

    pub fn snapshot(&self) -> &StoredSemanticPlan {
        &self.snapshot
    }

    pub fn is_document_owned_candidate(&self) -> bool {
        self.document_bytes.is_some()
    }

    pub fn document_bytes(&self) -> Option<&[u8]> {
        self.document_bytes.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanLayoutPoint {
    pub x: i32,
    pub y: i32,
}

/// Purely presentational data. Dependencies, callbacks and required resources
/// are not representable here. Missing/removed node references are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanLayoutDraft {
    pub layout_id: String,
    pub semantic_anchor: u64,
    pub node_positions: BTreeMap<String, PlanLayoutPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPlanLayout {
    pub plan_id: String,
    pub version: u64,
    pub layout: PlanLayoutDraft,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedSourceHandoff {
    plan_id: String,
    semantic_revision: u64,
    target: PlanAuthoringSource,
    inspection: Vec<u8>,
    document_bytes: Option<Vec<u8>>,
}

impl CheckedSourceHandoff {
    /// The trusted Rust app author explicitly authorizes a *fully representable*
    /// document, checked against both the real plan and operation descriptors.
    pub fn rust_to_document<H: ?Sized + Send + Sync>(
        original: &ValidatedExecutionPlan,
        document: &S2PlanDocument,
        handlers: &OperationRegistry<H>,
        limits: S2DocumentLimits,
    ) -> Result<Self, PlanRevisionError> {
        let coverage =
            PlanSourceCoverageReport::compare_rust_to_s2(original, document, handlers, limits);
        if !coverage.is_complete() {
            return Err(PlanRevisionError::Invalid(format!(
                "Rust to document transfer has {} unsupported or changed construct(s): {:?}",
                coverage.findings.len(),
                coverage.findings,
            )));
        }
        let candidate = PlanSemanticCandidate::from_s2(document, handlers, limits)?;
        let source = PlanSemanticCandidate::from_rust(original)?;
        if candidate.snapshot() != source.snapshot() {
            return Err(PlanRevisionError::Invalid(
                "Rust source cannot be represented by this S2 document".into(),
            ));
        }
        Ok(Self {
            plan_id: source.snapshot.plan_id,
            semantic_revision: source.snapshot.revision,
            target: PlanAuthoringSource::Document,
            inspection: source.snapshot.inspection,
            document_bytes: candidate.document_bytes,
        })
    }

    /// This must be invoked by the trusted application owner with its actual
    /// target Rust plan, never by a remote client claiming a Rust source.
    pub fn document_to_rust<H: ?Sized + Send + Sync>(
        current_document: &S2PlanDocument,
        target_rust: &ValidatedExecutionPlan,
        handlers: &OperationRegistry<H>,
        limits: S2DocumentLimits,
    ) -> Result<Self, PlanRevisionError> {
        let coverage = PlanSourceCoverageReport::compare_rust_to_s2(
            target_rust,
            current_document,
            handlers,
            limits,
        );
        if !coverage.is_complete() {
            return Err(PlanRevisionError::Invalid(format!(
                "Document to Rust transfer has {} unsupported or changed construct(s): {:?}",
                coverage.findings.len(),
                coverage.findings,
            )));
        }
        let candidate = PlanSemanticCandidate::from_s2(current_document, handlers, limits)?;
        let source = PlanSemanticCandidate::from_rust(target_rust)?;
        if candidate.snapshot() != source.snapshot() {
            return Err(PlanRevisionError::Invalid(
                "requested Rust source does not represent the current S2 plan".into(),
            ));
        }
        Ok(Self {
            plan_id: source.snapshot.plan_id,
            semantic_revision: source.snapshot.revision,
            target: PlanAuthoringSource::Rust,
            inspection: source.snapshot.inspection,
            document_bytes: None,
        })
    }

    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    pub fn revision(&self) -> u64 {
        self.semantic_revision
    }

    pub fn target(&self) -> PlanAuthoringSource {
        self.target
    }

    pub fn inspection(&self) -> &[u8] {
        &self.inspection
    }

    pub fn document_bytes(&self) -> Option<&[u8]> {
        self.document_bytes.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlanRevisionError {
    #[error("invalid V2 authoring request: {0}")]
    Invalid(String),
    #[error("missing plan, revision or layout")]
    NotFound,
    #[error("stale predecessor, source owner or artifact registration")]
    Conflict,
    #[error("unsupported persisted V2 store format: {0}")]
    UnsupportedFormat(i64),
    #[error("V2 revision repository failed: {0}")]
    Storage(String),
    #[error(transparent)]
    Candidate(#[from] S2DocumentError),
}

pub fn check_layout_nodes(
    draft: &PlanLayoutDraft,
    nodes: &[String],
) -> Result<(), PlanRevisionError> {
    if draft.layout_id.is_empty()
        || draft.layout_id.len() > 256
        || draft
            .layout_id
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        || draft.semantic_anchor == 0
        || draft.node_positions.len() > 4096
    {
        return Err(PlanRevisionError::Invalid(
            "invalid layout identity or size".into(),
        ));
    }
    let allowed: BTreeSet<&str> = nodes.iter().map(String::as_str).collect();
    if draft
        .node_positions
        .keys()
        .any(|node| !allowed.contains(node.as_str()))
    {
        return Err(PlanRevisionError::Invalid(
            "layout references an unknown node in its semantic anchor".into(),
        ));
    }
    let bytes =
        serde_json::to_vec(draft).map_err(|error| PlanRevisionError::Invalid(error.to_string()))?;
    if bytes.len() > MAX_STORED_LAYOUT_BYTES {
        return Err(PlanRevisionError::Invalid(
            "layout exceeds byte budget".into(),
        ));
    }
    Ok(())
}

/// Portable V2 conditional persistence protocol. Implemented by the local
/// SQLite adapter in `riesel_storage_sqlite`; the graph runner has no SQLite
/// dependency. These are trusted-local methods only, not remote authorization.
pub trait PlanRevisionRepository: Send + Sync {
    fn initialize(
        &self,
        candidate: &PlanSemanticCandidate,
        source: PlanAuthoringSource,
        registration_stamp: &str,
    ) -> Result<PlanAuthoringHead, PlanRevisionError>;

    fn head(&self, plan_id: &str) -> Result<PlanAuthoringHead, PlanRevisionError>;

    fn semantic(
        &self,
        plan_id: &str,
        revision: u64,
    ) -> Result<StoredSemanticPlan, PlanRevisionError>;

    /// Immutable semantic revision. The old active runtime (if any) is NOT
    /// changed; activation is a separate application-owned #195 operation.
    fn append(
        &self,
        expected: &PlanAuthoringHead,
        next: &PlanSemanticCandidate,
    ) -> Result<PlanAuthoringHead, PlanRevisionError>;

    /// Layout revision is independent from the semantic-head predecessor.
    /// `expected_version=None` creates one new layout stream at version 1.
    fn save_layout(
        &self,
        expected: &PlanAuthoringHead,
        expected_version: Option<u64>,
        draft: &PlanLayoutDraft,
    ) -> Result<StoredPlanLayout, PlanRevisionError>;

    fn layout(
        &self,
        plan_id: &str,
        layout_id: &str,
        version: u64,
    ) -> Result<StoredPlanLayout, PlanRevisionError>;

    /// Transfer only after app-owned representability proof. A new source
    /// generation fences all previous semantic and layout writers at commit.
    fn handoff(
        &self,
        expected: &PlanAuthoringHead,
        proof: &CheckedSourceHandoff,
    ) -> Result<PlanAuthoringHead, PlanRevisionError>;
}
