//! Inspect-only, per-node explanation of Rust ↔ S2-document representability.
//!
//! This compares the **real** validated source graph to an A2-bound candidate,
//! without executing Rust callbacks or granting a document source authority.
//! A successful report does not authenticate the caller or publish a revision.

use crate::{
    AccessMode, DomainAdmissionMode, OperationRegistry, OperationTrigger,
    S2DocumentAccessMode, S2DocumentDomainMode, S2DocumentLimits, S2DocumentTrigger,
    S2PlanDocument, ValidatedExecutionPlan, bind_s2_document,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceCoverageProblem {
    PlanIdentityMismatch,
    SemanticRevisionMismatch,
    MissingNode,
    ExtraNode,
    BindingMismatch,
    MissingEditorDescriptor,
    DescriptorVersionMismatch,
    ControlDependenciesChanged,
    TriggerChanged,
    ResourceAccessChanged,
    DomainAdmissionChanged,
    RejectedByPreflight(String),
    SemanticProjectionMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCoverageFinding {
    /// `None` denotes a whole-plan rather than a node-local error.
    pub node_id: Option<String>,
    pub problem: SourceCoverageProblem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSourceCoverageReport {
    pub plan_id: String,
    pub semantic_revision: u64,
    pub findings: Vec<SourceCoverageFinding>,
}

impl PlanSourceCoverageReport {
    pub fn is_complete(&self) -> bool {
        self.findings.is_empty()
    }

    /// Report all inspectable source discrepancies before any owner handoff.
    /// The final whole-plan exact-version validator/binder remains mandatory.
    pub fn compare_rust_to_s2<H: ?Sized + Send + Sync>(
        original: &ValidatedExecutionPlan,
        document: &S2PlanDocument,
        registry: &OperationRegistry<H>,
        limits: S2DocumentLimits,
    ) -> Self {
        let source = original.description();
        let mut report = Self {
            plan_id: source.id().to_owned(),
            semantic_revision: source.revision(),
            findings: Vec::new(),
        };
        if document.plan_id != source.id() {
            report.add(None, SourceCoverageProblem::PlanIdentityMismatch);
        }
        if document.semantic_revision != source.revision() {
            report.add(None, SourceCoverageProblem::SemanticRevisionMismatch);
        }

        for code_node in source.operations() {
            let Some(candidate) = document
                .operations
                .iter()
                .find(|node| node.id == code_node.id())
            else {
                report.add(Some(code_node.id()), SourceCoverageProblem::MissingNode);
                continue;
            };
            if candidate.binding.name != code_node.binding().name()
                || candidate.binding.version != code_node.binding().version()
            {
                report.add(Some(code_node.id()), SourceCoverageProblem::BindingMismatch);
            }
            if let Some(descriptor) = registry.editor_descriptor(code_node.binding()) {
                if candidate.binding.descriptor_version != descriptor.descriptor_version() {
                    report.add(
                        Some(code_node.id()),
                        SourceCoverageProblem::DescriptorVersionMismatch,
                    );
                }
            } else {
                report.add(
                    Some(code_node.id()),
                    SourceCoverageProblem::MissingEditorDescriptor,
                );
            }
            if candidate.dependencies.as_slice() != code_node.dependencies() {
                report.add(
                    Some(code_node.id()),
                    SourceCoverageProblem::ControlDependenciesChanged,
                );
            }
            let trigger_matches = match (code_node.trigger(), &candidate.trigger) {
                (OperationTrigger::Immediate, S2DocumentTrigger::Immediate) => true,
                (OperationTrigger::Signal(left), S2DocumentTrigger::Signal(right)) => left == right,
                _ => false,
            };
            if !trigger_matches {
                report.add(Some(code_node.id()), SourceCoverageProblem::TriggerChanged);
            }
            let resources_match = candidate.resources.len() == code_node.resources().len()
                && code_node
                    .resources()
                    .iter()
                    .zip(&candidate.resources)
                    .all(|(code, editor)| {
                        code.resource() == editor.resource
                            && matches!(
                                (code.mode(), editor.mode),
                                (AccessMode::Read, S2DocumentAccessMode::Read)
                                    | (AccessMode::Write, S2DocumentAccessMode::Write)
                            )
                    });
            if !resources_match {
                report.add(
                    Some(code_node.id()),
                    SourceCoverageProblem::ResourceAccessChanged,
                );
            }
            let domain_matches = match (code_node.domain_boundary(), &candidate.domain) {
                (None, None) => true,
                (Some(code), Some(editor)) => {
                    code.domain() == editor.name
                        && matches!(
                            (code.mode(), editor.mode),
                            (DomainAdmissionMode::RieselTick, S2DocumentDomainMode::RieselTick)
                                | (
                                    DomainAdmissionMode::DelegatedFrame,
                                    S2DocumentDomainMode::DelegatedFrame
                                )
                        )
                }
                _ => false,
            };
            if !domain_matches {
                report.add(
                    Some(code_node.id()),
                    SourceCoverageProblem::DomainAdmissionChanged,
                );
            }
        }

        for editor_node in &document.operations {
            if !source.operations().iter().any(|code| code.id() == editor_node.id) {
                report.add(
                    Some(&editor_node.id),
                    SourceCoverageProblem::ExtraNode,
                );
            }
        }

        match bind_s2_document(document, registry, limits) {
            Err(error) => report.add(
                None,
                SourceCoverageProblem::RejectedByPreflight(error.to_string()),
            ),
            Ok(bound) => {
                match (
                    serde_json::to_vec(source),
                    serde_json::to_vec(bound.plan().description()),
                ) {
                    (Ok(code), Ok(candidate)) if code == candidate => {}
                    (Ok(_), Ok(_)) => {
                        report.add(None, SourceCoverageProblem::SemanticProjectionMismatch);
                    }
                    (Err(error), _) | (_, Err(error)) => {
                        report.add(
                            None,
                            SourceCoverageProblem::RejectedByPreflight(error.to_string()),
                        );
                    }
                }
            }
        }
        report
    }

    fn add(&mut self, node_id: Option<&str>, problem: SourceCoverageProblem) {
        self.findings.push(SourceCoverageFinding {
            node_id: node_id.map(str::to_owned),
            problem,
        });
    }
}
