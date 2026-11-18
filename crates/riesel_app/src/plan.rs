//! Declarative startup plan. Validation must not execute components, open
//! sockets, or construct local providers. Runtime connection verification
//! still occurs when each selected service actually starts.

use riesel_schema::CapabilityRef;
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Deterministically ordered, locally validated deployment requirements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeploymentPlan {
    pub services: Vec<PlannedService>,
}

/// An application capability alias and the provider target selected for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedService {
    pub requirement: String,
    pub capability: CapabilityRef,
    pub depends_on: Vec<String>,
    pub target: PlannedTarget,
}

/// Provider targets known before execution. For TCP and managed targets,
/// remote identity and contract cannot be verified until a connection exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum PlannedTarget {
    Local { provider: String },
    Tcp { address: SocketAddr },
    Managed {
        program: PathBuf,
        startup_timeout_ms: u64,
        shutdown_timeout_ms: u64,
    },
}
