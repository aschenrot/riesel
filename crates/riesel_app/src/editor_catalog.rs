//! Trusted-application-owned metadata for bounded S2 visual plan authoring.
//!
//! This is NOT a document parser, artifact authenticator, permission grant,
//! portable Rust callback ABI, or a general typed-dataflow engine.
//! Only the trusted application can register a handler as editor-selectable.
//! Exact-version binding is performed by the existing execution-plan model.

use crate::execution_plan::{
    AccessMode, BoundExecutionPlan, DomainAdmissionMode, ExecutionPlanError, OperationKey,
    OperationNode, OperationRegistry, OperationTrigger, ResourceAccess, ValidatedExecutionPlan,
};
use serde::Serialize;
use std::{collections::BTreeSet, sync::Arc};
use thiserror::Error;

/// A restriction that must be understood before an editor may bind an operation.
///
/// The current graph runner cannot prove native-thread-affine callback safety.
/// Such operations are explicitly excluded from editable binding until an
/// independently verified owner/affinity adapter exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorAffinity {
    SessionOwned,
    NativeThread,
}

/// Publisher-attested, passive authoring metadata for one exact registered key.
///
/// This is supplied by trusted application Rust code at handler registration.
/// It is intentionally Serialize-only; untrusted documents cannot create
/// descriptors, change mandatory resources, or grant themselves editability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditorOperationDescriptor {
    key: OperationKey,
    required_access: Vec<ResourceAccess>,
    permitted_extra_access: Vec<ResourceAccess>,
    permitted_triggers: Vec<OperationTrigger>,
    required_domain: Option<(String, DomainAdmissionMode)>,
    affinity: EditorAffinity,
}

impl EditorOperationDescriptor {
    pub fn new(key: OperationKey) -> Self {
        Self {
            key,
            required_access: Vec::new(),
            permitted_extra_access: Vec::new(),
            permitted_triggers: vec![OperationTrigger::Immediate],
            required_domain: None,
            affinity: EditorAffinity::SessionOwned,
        }
    }

    /// An operation's compulsory access is never editable or removable.
    pub fn require_access(mut self, resource: impl Into<String>, mode: AccessMode) -> Self {
        self.required_access.push(ResourceAccess::new(resource, mode));
        self
    }

    /// Additional access may be selected but never weakens required access.
    pub fn permit_extra_access(mut self, resource: impl Into<String>, mode: AccessMode) -> Self {
        self.permitted_extra_access
            .push(ResourceAccess::new(resource, mode));
        self
    }

    pub fn permit_signal(mut self, signal: impl Into<String>) -> Self {
        self.permitted_triggers
            .push(OperationTrigger::Signal(signal.into()));
        self
    }

    /// Fixed, owner-authoritative domain admission for this operation.
    /// It cannot be changed to a different domain or admission owner.
    pub fn require_domain(
        mut self,
        domain: impl Into<String>,
        mode: DomainAdmissionMode,
    ) -> Self {
        self.required_domain = Some((domain.into(), mode));
        self
    }

    pub fn affinity(mut self, affinity: EditorAffinity) -> Self {
        self.affinity = affinity;
        self
    }

    pub fn key(&self) -> &OperationKey {
        &self.key
    }

    pub fn required_access(&self) -> &[ResourceAccess] {
        &self.required_access
    }

    pub fn permitted_extra_access(&self) -> &[ResourceAccess] {
        &self.permitted_extra_access
    }

    pub fn permitted_triggers(&self) -> &[OperationTrigger] {
        &self.permitted_triggers
    }

    pub fn required_domain(&self) -> Option<(&str, DomainAdmissionMode)> {
        self.required_domain
            .as_ref()
            .map(|(name, mode)| (name.as_str(), *mode))
    }

    pub fn execution_affinity(&self) -> EditorAffinity {
        self.affinity
    }

    fn validate(&self) -> Result<(), EditorCatalogError> {
        if self.affinity == EditorAffinity::NativeThread {
            return Err(EditorCatalogError::UnsupportedAffinity {
                name: self.key.name().to_owned(),
            });
        }
        if let Some((domain, _)) = &self.required_domain
            && !valid_name(domain)
        {
            return Err(invalid(&self.key, "invalid compulsory domain"));
        }
        let mut named = BTreeSet::new();
        for access in self
            .required_access
            .iter()
            .chain(self.permitted_extra_access.iter())
        {
            if !valid_name(access.resource()) || !named.insert(access.resource()) {
                return Err(invalid(&self.key, "invalid or repeated resource permission"));
            }
        }
        let mut triggers = BTreeSet::new();
        for trigger in &self.permitted_triggers {
            let value = match trigger {
                OperationTrigger::Immediate => "immediate".to_owned(),
                OperationTrigger::Signal(signal) if valid_name(signal) => {
                    format!("signal:{signal}")
                }
                OperationTrigger::Signal(_) => return Err(invalid(&self.key, "invalid signal")),
            };
            if !triggers.insert(value) {
                return Err(invalid(&self.key, "duplicate trigger permission"));
            }
        }
        if self.permitted_triggers.is_empty() {
            return Err(invalid(&self.key, "no permitted trigger"));
        }
        Ok(())
    }

    fn validate_node(&self, node: &OperationNode) -> Result<(), EditorCatalogError> {
        if self.affinity != EditorAffinity::SessionOwned {
            return Err(EditorCatalogError::UnsupportedAffinity {
                name: self.key.name().to_owned(),
            });
        }
        if !self.permitted_triggers.iter().any(|item| item == node.trigger()) {
            return Err(violation(node, "unapproved trigger"));
        }
        match (self.required_domain(), node.domain_boundary()) {
            (None, None) => {}
            (Some((name, mode)), Some(actual))
                if name == actual.domain() && mode == actual.mode() => {}
            _ => return Err(violation(node, "domain admission owner mismatch")),
        }
        for access in &self.required_access {
            if !node.resources().iter().any(|actual| actual == access) {
                return Err(violation(node, "compulsory resource access omitted or weakened"));
            }
        }
        for access in node.resources() {
            if !self.required_access.contains(access)
                && !self.permitted_extra_access.contains(access)
            {
                return Err(violation(node, "unapproved resource access"));
            }
        }
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && !name.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

fn invalid(key: &OperationKey, reason: &str) -> EditorCatalogError {
    EditorCatalogError::InvalidDescriptor {
        name: key.name().to_owned(),
        reason: reason.to_owned(),
    }
}

fn violation(node: &OperationNode, reason: &str) -> EditorCatalogError {
    EditorCatalogError::DisallowedNode {
        node: node.id().to_owned(),
        reason: reason.to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EditorCatalogError {
    #[error("invalid authoring descriptor for {name}: {reason}")]
    InvalidDescriptor { name: String, reason: String },
    #[error("editable native-thread-affine operation {name} is not supported")]
    UnsupportedAffinity { name: String },
    #[error("node {node} binds a non-editor-authorized operation {name} version {version}")]
    NotEditable {
        node: String,
        name: String,
        version: u32,
    },
    #[error("node {node} violates the publisher's editing rules: {reason}")]
    DisallowedNode { node: String, reason: String },
    #[error(transparent)]
    Plan(#[from] ExecutionPlanError),
}

impl<H: ?Sized> OperationRegistry<H> {
    /// Register real application-owned Rust behavior and an inert descriptor
    /// together. Ordinary `register` remains code-first and non-editable.
    pub fn register_editor_operation(
        &mut self,
        descriptor: EditorOperationDescriptor,
        handler: Arc<H>,
    ) -> Result<(), EditorCatalogError> {
        descriptor.validate()?;
        let key = descriptor.key.clone();
        self.register(key.clone(), handler)?;
        self.editable_operations.insert(key, descriptor);
        Ok(())
    }

    /// Read-only descriptor: no callback invocation or provider startup.
    pub fn editor_descriptor(&self, key: &OperationKey) -> Option<&EditorOperationDescriptor> {
        self.editable_operations.get(key)
    }

    /// Passive publication of *trusted* descriptor copies. Not a credential,
    /// signed artifact manifest or independently authorized remote exporter.
    pub fn editor_descriptors(&self) -> Vec<&EditorOperationDescriptor> {
        self.editable_operations.values().collect()
    }

    /// Require exact trusted descriptor policy before using ordinary handler
    /// binding. Never use `ValidatedExecutionPlan::bind` alone for untrusted
    /// document-owned S2 candidates.
    pub fn bind_editor_plan(
        &self,
        plan: &ValidatedExecutionPlan,
    ) -> Result<BoundExecutionPlan<H>, EditorCatalogError> {
        for node in plan.description().operations() {
            let Some(descriptor) = self.editable_operations.get(node.binding()) else {
                return Err(EditorCatalogError::NotEditable {
                    node: node.id().to_owned(),
                    name: node.binding().name().to_owned(),
                    version: node.binding().version(),
                });
            };
            descriptor.validate_node(node)?;
        }
        Ok(plan.bind(self)?)
    }
}
