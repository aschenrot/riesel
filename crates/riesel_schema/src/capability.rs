use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Identifies a versioned capability contract, not a provider or process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRef {
    pub id: String,
    pub version: u32,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapabilityRefError {
    #[error("invalid capability identifier: {0}")]
    InvalidId(String),
    #[error("capability version must be greater than zero")]
    InvalidVersion,
}

impl CapabilityRef {
    pub fn validate(&self) -> Result<(), CapabilityRefError> {
        if !valid_capability_id(&self.id) {
            return Err(CapabilityRefError::InvalidId(self.id.clone()));
        }
        if self.version == 0 {
            return Err(CapabilityRefError::InvalidVersion);
        }
        Ok(())
    }
}

pub(crate) fn valid_capability_id(id: &str) -> bool {
    let mut segments = id.split('.');
    if segments.clone().count() < 2 {
        return false;
    }
    segments.all(|part| {
        let mut chars = part.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_ref_round_trips_through_json() {
        let original = CapabilityRef {
            id: "riesel.storage.records".into(),
            version: 1,
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: CapabilityRef = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, original);
        restored.validate().unwrap();
    }

    #[test]
    fn capability_ref_rejects_unknown_fields() {
        let json = r#"{"id":"riesel.storage.records","version":1,"address":"127.0.0.1"}"#;
        assert!(serde_json::from_str::<CapabilityRef>(json).is_err());
    }

    #[test]
    fn capability_ref_rejects_invalid_ids_and_versions() {
        let mut reference = CapabilityRef {
            id: "riesel.storage.records".into(),
            version: 0,
        };
        assert_eq!(
            reference.validate(),
            Err(CapabilityRefError::InvalidVersion)
        );

        reference.version = 1;
        for id in [
            "storage",
            "Riesel.storage",
            "riesel..storage",
            "riesel.storage-raw",
        ] {
            reference.id = id.into();
            assert_eq!(
                reference.validate(),
                Err(CapabilityRefError::InvalidId(id.into()))
            );
        }
    }
}
