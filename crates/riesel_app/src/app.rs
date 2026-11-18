use crate::AppConfig;
use riesel_schema::{AppSchema, SchemaError, parse_schema};
use thiserror::Error;

pub struct App {
    config: AppConfig,
    definition: Option<AppSchema>,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Unknown error")]
    Unknown,
    #[error("cannot read application definition: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid application JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid application definition: {0}")]
    Schema(#[from] SchemaError),
}

impl App {
    pub fn new() -> App {
        Self::with_config(AppConfig::default())
    }

    pub fn with_config(config: AppConfig) -> App {
        App {
            config,
            definition: None,
        }
    }

    /// Load and validate an application definition. This does not start services.
    pub fn load(path: &str) -> Result<App, AppError> {
        let source = std::fs::read_to_string(path)?;
        Self::from_schema(&source)
    }

    /// Construct an application from validated schema JSON without file I/O.
    pub fn from_schema(source: &str) -> Result<App, AppError> {
        let definition = parse_schema(source)?;
        definition.validate()?;
        let config = AppConfig {
            name: definition.app.name.clone(),
        };
        Ok(App {
            config,
            definition: Some(definition),
        })
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn definition(&self) -> Option<&AppSchema> {
        self.definition.as_ref()
    }

    pub fn run(self) -> Result<(), AppError> {
        Ok(())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_DEFINITION: &str = r#"{
        "version": 1,
        "app": {"name": "Todo"},
        "models": {},
        "requires": [
            {"name": "storage", "capability": {"id": "riesel.storage.records", "version": 1}}
        ]
    }"#;

    #[test]
    fn constructs_app_from_valid_definition() {
        let app = App::from_schema(VALID_DEFINITION).unwrap();
        assert_eq!(app.config().name, "Todo");
        assert_eq!(app.definition().unwrap().requires.len(), 1);
        assert!(App::new().definition().is_none());
    }

    #[test]
    fn rejects_invalid_json_and_unsupported_schema() {
        assert!(matches!(App::from_schema("{"), Err(AppError::Json(_))));
        let invalid = r#"{"version":2,"app":{"name":"Todo"},"models":{}}"#;
        assert!(matches!(
            App::from_schema(invalid),
            Err(AppError::Schema(SchemaError::UnsupportedVersion { version: 2 }))
        ));
    }

    #[test]
    fn loads_definition_from_disk() {
        let path = std::env::temp_dir().join(format!(
            "riesel_app_test_{}_{}.json",
            std::process::id(),
            line!()
        ));
        std::fs::write(&path, VALID_DEFINITION).unwrap();
        let loaded = App::load(path.to_str().unwrap());
        std::fs::remove_file(path).unwrap();

        let app = loaded.unwrap();
        assert_eq!(app.config().name, "Todo");
        assert!(app.definition().is_some());
    }

    #[test]
    fn reports_missing_application_file() {
        assert!(matches!(App::load(""), Err(AppError::Io(_))));
    }
}
