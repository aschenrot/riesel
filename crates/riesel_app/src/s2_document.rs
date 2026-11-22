//! Bounded, strict S2 **candidate** authoring documents.
//!
//! Unlike the Serialize-only ExecutionPlanDescription inspection projection,
//! these bytes are explicitly imported as untrusted, versioned source. Neither
//! parsing nor exact binding executes a handler or authorizes publication.
//! Future executable features are opaque/read-only to this S2 reader.

use crate::{
    AccessMode, BoundExecutionPlan, DomainAdmissionMode, EditorCatalogError, ExecutionPlanDraft,
    ExecutionPlanError, OperationKey, OperationNode, OperationRegistry, ValidatedExecutionPlan,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

pub const S2_DOCUMENT_VERSION: u32 = 1;

/// Conservative bounded default for untrusted local authoring input.
/// Applications may deliberately supply larger budgets after evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S2DocumentLimits {
    pub max_bytes: usize,
    pub max_nodes: usize,
    pub max_dependencies: usize,
    pub max_resource_entries: usize,
}

impl Default for S2DocumentLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024,
            max_nodes: 4096,
            max_dependencies: 16384,
            max_resource_entries: 16384,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S2PlanDocument {
    pub format_version: u32,
    pub required_features: Vec<String>,
    pub plan_id: String,
    pub semantic_revision: u64,
    pub operations: Vec<S2OperationDocument>,
}

impl S2PlanDocument {
    pub fn new(plan_id: impl Into<String>, semantic_revision: u64) -> Self {
        Self {
            format_version: S2_DOCUMENT_VERSION,
            required_features: Vec::new(),
            plan_id: plan_id.into(),
            semantic_revision,
            operations: Vec::new(),
        }
    }

    pub fn operation(mut self, operation: S2OperationDocument) -> Self {
        self.operations.push(operation);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S2OperationBinding {
    pub name: String,
    pub version: u32,
    /// Authoring schema version, distinct from the Rust callback version.
    pub descriptor_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "signal", rename_all = "snake_case")]
pub enum S2DocumentTrigger {
    Immediate,
    Signal(String),
}

impl Default for S2DocumentTrigger {
    fn default() -> Self {
        Self::Immediate
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum S2DocumentAccessMode {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S2DocumentAccess {
    pub resource: String,
    pub mode: S2DocumentAccessMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum S2DocumentDomainMode {
    RieselTick,
    DelegatedFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S2DocumentDomain {
    pub name: String,
    pub mode: S2DocumentDomainMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S2OperationDocument {
    pub id: String,
    pub binding: S2OperationBinding,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub trigger: S2DocumentTrigger,
    #[serde(default)]
    pub resources: Vec<S2DocumentAccess>,
    #[serde(default)]
    pub domain: Option<S2DocumentDomain>,
}

impl S2OperationDocument {
    pub fn new(
        id: impl Into<String>,
        key: &OperationKey,
        descriptor_version: u32,
    ) -> Self {
        Self {
            id: id.into(),
            binding: S2OperationBinding {
                name: key.name().to_owned(),
                version: key.version(),
                descriptor_version,
            },
            dependencies: Vec::new(),
            trigger: S2DocumentTrigger::Immediate,
            resources: Vec::new(),
            domain: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S2SourceDocument {
    /// Parsed and understood S2 source. A separately checked binding is still
    /// required before saving an executable successor or publishing it.
    Editable(S2PlanDocument),
    /// Original bytes retained intact; no writable document is exposed.
    UnsupportedReadOnly {
        original: Vec<u8>,
        reason: String,
    },
}

impl S2SourceDocument {
    pub fn editable_mut(&mut self) -> Result<&mut S2PlanDocument, S2DocumentError> {
        match self {
            Self::Editable(document) => Ok(document),
            Self::UnsupportedReadOnly { .. } => Err(S2DocumentError::UnsupportedReadOnly),
        }
    }

    pub fn original_unsupported_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Editable(_) => None,
            Self::UnsupportedReadOnly { original, .. } => Some(original),
        }
    }

    /// A safe authoring save path: never serialize a future unknown executable
    /// document as if it were an editable S2 source.
    pub fn serialize_checked<H: ?Sized + Send + Sync>(
        &self,
        registry: &OperationRegistry<H>,
        limits: S2DocumentLimits,
    ) -> Result<Vec<u8>, S2DocumentError> {
        let Self::Editable(document) = self else {
            return Err(S2DocumentError::UnsupportedReadOnly);
        };
        bind_s2_document(document, registry, limits)?;
        let bytes = serde_json::to_vec(document)
            .map_err(|error| S2DocumentError::InvalidSource(error.to_string()))?;
        if bytes.len() > limits.max_bytes {
            return Err(S2DocumentError::LimitExceeded("serialized byte budget"));
        }
        Ok(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum S2DocumentError {
    #[error("invalid S2 executable source: {0}")]
    InvalidSource(String),
    #[error("S2 document exceeds {0}")]
    LimitExceeded(&'static str),
    #[error("unsupported S2 document is read-only; it cannot be edited or saved")]
    UnsupportedReadOnly,
    #[error("operation {node} requires editor descriptor version {expected}, got {actual}")]
    DescriptorVersionMismatch {
        node: String,
        expected: u32,
        actual: u32,
    },
    #[error("operation {node} has no application-owned registered editor descriptor")]
    MissingEditorDescriptor { node: String },
    #[error(transparent)]
    InvalidPlan(#[from] ExecutionPlanError),
    #[error(transparent)]
    Disallowed(#[from] EditorCatalogError),
}

impl fmt::Display for S2SourceDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Editable(document) => {
                write!(formatter, "editable S2 plan {}", document.plan_id)
            }
            Self::UnsupportedReadOnly { reason, .. } => {
                write!(formatter, "unsupported read-only source: {reason}")
            }
        }
    }
}

fn ensure_limits(
    document: &S2PlanDocument,
    limits: S2DocumentLimits,
) -> Result<(), S2DocumentError> {
    if document.operations.len() > limits.max_nodes {
        return Err(S2DocumentError::LimitExceeded("node budget"));
    }
    let dependencies = document
        .operations
        .iter()
        .fold(0usize, |sum, node| sum.saturating_add(node.dependencies.len()));
    if dependencies > limits.max_dependencies {
        return Err(S2DocumentError::LimitExceeded("control dependency budget"));
    }
    let resources = document
        .operations
        .iter()
        .fold(0usize, |sum, node| sum.saturating_add(node.resources.len()));
    if resources > limits.max_resource_entries {
        return Err(S2DocumentError::LimitExceeded("resource declaration budget"));
    }
    Ok(())
}

/// Classify executable source without silently stripping unknown S3-required
/// dataflow features. For an unsupported format/feature, preserve exact input
/// bytes but offer **no editable document**.
pub fn inspect_s2_source(
    original: &[u8],
    limits: S2DocumentLimits,
) -> Result<S2SourceDocument, S2DocumentError> {
    if original.len() > limits.max_bytes {
        return Err(S2DocumentError::LimitExceeded("source byte budget"));
    }
    let value: serde_json::Value = serde_json::from_slice(original)
        .map_err(|error| S2DocumentError::InvalidSource(error.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| S2DocumentError::InvalidSource("expected a document object".into()))?;
    let version = object
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| S2DocumentError::InvalidSource("missing format version".into()))?;
    let features = object
        .get("required_features")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| S2DocumentError::InvalidSource("missing required features".into()))?;
    if !features.iter().all(serde_json::Value::is_string) {
        return Err(S2DocumentError::InvalidSource(
            "required features must be strings".into(),
        ));
    }
    if version != u64::from(S2_DOCUMENT_VERSION) || !features.is_empty() {
        return Ok(S2SourceDocument::UnsupportedReadOnly {
            original: original.to_vec(),
            reason: format!(
                "unsupported executable format {version} or required features"
            ),
        });
    }
    // Decode from the ORIGINAL bytes, not from the Value; this also makes
    // duplicate executable keys fail instead of silently overwriting them.
    let document: S2PlanDocument = serde_json::from_slice(original)
        .map_err(|error| S2DocumentError::InvalidSource(error.to_string()))?;
    ensure_limits(&document, limits)?;
    Ok(S2SourceDocument::Editable(document))
}

/// Construct the actual Riesel plan and bind it against the **registered
/// publisher-owned descriptors**; never execute or publish a callback.
pub fn bind_s2_document<H: ?Sized + Send + Sync>(
    document: &S2PlanDocument,
    registry: &OperationRegistry<H>,
    limits: S2DocumentLimits,
) -> Result<BoundExecutionPlan<H>, S2DocumentError> {
    if document.format_version != S2_DOCUMENT_VERSION || !document.required_features.is_empty() {
        return Err(S2DocumentError::UnsupportedReadOnly);
    }
    ensure_limits(document, limits)?;
    let mut plan = ExecutionPlanDraft::new(document.plan_id.as_str(), document.semantic_revision);
    for operation in &document.operations {
        let key = OperationKey::new(operation.binding.name.as_str(), operation.binding.version)?;
        let descriptor = registry.editor_descriptor(&key).ok_or_else(|| {
            S2DocumentError::MissingEditorDescriptor {
                node: operation.id.clone(),
            }
        })?;
        if operation.binding.descriptor_version != descriptor.descriptor_version() {
            return Err(S2DocumentError::DescriptorVersionMismatch {
                node: operation.id.clone(),
                expected: descriptor.descriptor_version(),
                actual: operation.binding.descriptor_version,
            });
        }
        let mut node = OperationNode::new(operation.id.as_str(), key);
        for dependency in &operation.dependencies {
            node = node.after(dependency.as_str());
        }
        if let S2DocumentTrigger::Signal(signal) = &operation.trigger {
            node = node.on_signal(signal.as_str());
        }
        for access in &operation.resources {
            let mode = match access.mode {
                S2DocumentAccessMode::Read => AccessMode::Read,
                S2DocumentAccessMode::Write => AccessMode::Write,
            };
            node = node.access(access.resource.as_str(), mode);
        }
        if let Some(domain) = &operation.domain {
            node = match domain.mode {
                S2DocumentDomainMode::RieselTick => node.domain_tick(domain.name.as_str()),
                S2DocumentDomainMode::DelegatedFrame => node.delegated_frame(domain.name.as_str()),
            };
        }
        plan = plan.operation(node);
    }
    let validated: ValidatedExecutionPlan = plan.validate()?;
    Ok(registry.bind_editor_plan(&validated)?)
}
