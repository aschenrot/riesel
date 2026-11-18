use serde::{Deserialize, Serialize};

/// Identifies a versioned capability contract, not a provider or process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRef {
    pub id: String,
    pub version: u32,
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
    }

    #[test]
    fn capability_ref_rejects_unknown_fields() {
        let json = r#"{"id":"riesel.storage.records","version":1,"address":"127.0.0.1"}"#;

        assert!(serde_json::from_str::<CapabilityRef>(json).is_err());
    }
}
