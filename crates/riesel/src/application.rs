use crate::{Records, RieselModel, TcpRecordStoreConnector};
use riesel_app::{
    App, AppRunError, ConnectorError, ConnectorRegistry, DeploymentConfig, LocalProviderFactory,
    RunningApp, RuntimeError,
};
use riesel_schema::{AppDefinition, AppSchema, CapabilityRequirement};
use riesel_storage::{RecordStore, RecordStoreContract};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SdkError {
    #[error(transparent)]
    App(#[from] riesel_app::AppError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Execution(#[from] AppRunError),
    #[error(transparent)]
    Contract(#[from] riesel_schema::ContractError),
    #[error(transparent)]
    Store(#[from] riesel_storage::StoreError),
    #[error(transparent)]
    RecordValidation(#[from] riesel_schema::RecordError),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Deployment(#[from] riesel_app::DeploymentError),
    #[error("model {0} has already been registered with different metadata")]
    DuplicateModel(String),
    #[error("model {0} was not registered")]
    MissingModel(String),
    #[error("Rust model {0} does not match the application metadata")]
    ModelMismatch(String),
    #[error("record-storage capability is not declared")]
    MissingStorage,
    #[error("multiple record-storage requirements are declared")]
    AmbiguousStorage,
    #[error("the storage requirement name is already used for a different capability")]
    RequirementConflict,
    #[error("invalid application arguments: {0}")]
    Arguments(String),
}

pub type Result<T> = std::result::Result<T, SdkError>;

/// Rust-first application authoring. Low-level App remains available for
/// external schemas and generated definitions.
pub struct Application {
    definition: AppSchema,
    providers: ConnectorRegistry,
    deployment: Option<DeploymentConfig>,
}

impl Application {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            definition: AppSchema {
                version: 1,
                app: AppDefinition { name: name.into() },
                models: BTreeMap::new(),
                requires: vec![],
            },
            providers: ConnectorRegistry::new(),
            deployment: None,
        }
    }

    /// Register an authoritative Rust model and its record-storage requirement.
    pub fn records<T: RieselModel>(mut self) -> Result<Self> {
        let name = T::MODEL_NAME.to_owned();
        let definition = T::model_definition();
        if let Some(previous) = self.definition.models.get(&name) {
            if previous != &definition {
                return Err(SdkError::DuplicateModel(name));
            }
        } else {
            self.definition.models.insert(name, definition);
        }

        let capability = RecordStoreContract::definition()?.capability;
        if let Some(existing) = self.definition.requires.iter().find(|req| req.name == "storage") {
            if existing.capability != capability {
                return Err(SdkError::RequirementConflict);
            }
        } else {
            self.definition.requires.push(CapabilityRequirement {
                name: "storage".into(),
                capability,
            });
            // Register the transport adapter only once, without connecting.
            self.providers.register(TcpRecordStoreConnector)?;
        }
        Ok(self)
    }

    /// Choose a local provider without requiring deployment or a transport.
    pub fn provider(mut self, factory: impl LocalProviderFactory + 'static) -> Result<Self> {
        self.providers.register_local(factory)?;
        Ok(self)
    }

    /// Explicit opt-in to ephemeral in-memory storage for development.
    /// Production applications never fall back to MemoryStore automatically.
    #[cfg(feature = "dev-memory-storage")]
    pub fn development(self) -> Result<Self> {
        self.provider(riesel_storage_component::MemoryStoreFactory)
    }

    /// Configure an explicit local provider or external/managed service.
    pub fn deployment(mut self, deployment: DeploymentConfig) -> Self {
        self.deployment = Some(deployment);
        self
    }

    pub fn definition(&self) -> &AppSchema {
        &self.definition
    }

    pub fn run(self, entrypoint: impl FnOnce(&Context<'_>) -> Result<()>) -> Result<()> {
        let app = App::from_definition(self.definition)?;
        let invoke = |running: &RunningApp| -> std::result::Result<(), ConnectorError> {
            entrypoint(&Context { running })
                .map_err(|error| Box::new(error) as ConnectorError)
        };
        if let Some(deployment) = self.deployment {
            app.run(&deployment, &self.providers, invoke)?;
        } else {
            app.run_local(&self.providers, invoke)?;
        }
        Ok(())
    }

    /// Parse infrastructure flags, keeping application-specific commands separate.
    /// Supported: --deployment FILE and --help.
    pub fn run_from_env(
        mut self,
        entrypoint: impl FnOnce(&Context<'_>) -> Result<()>,
    ) -> Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        match args.as_slice() {
            [] => {}
            [option] if option == "--help" || option == "-h" => {
                println!("Usage: <application> [--deployment FILE]");
                return Ok(());
            }
            [option, path] if option == "--deployment" => {
                self.deployment = Some(DeploymentConfig::load(PathBuf::from(path))?);
            }
            _ => {
                return Err(SdkError::Arguments(
                    "expected --deployment FILE or --help".into(),
                ));
            }
        }
        self.run(entrypoint)
    }
}

/// Application-level service access. Callers never downcast or choose
/// transport-specific trait object types.
pub struct Context<'a> {
    running: &'a RunningApp,
}

impl Context<'_> {
    pub fn records<T>(&self) -> Result<Records<T>>
    where
        T: RieselModel + Serialize + DeserializeOwned,
    {
        let schema = self.running.app().definition().ok_or(SdkError::MissingStorage)?;
        let registered = schema.models
            .get(T::MODEL_NAME)
            .ok_or_else(|| SdkError::MissingModel(T::MODEL_NAME.into()))?;
        if registered != &T::model_definition() {
            return Err(SdkError::ModelMismatch(T::MODEL_NAME.into()));
        }

        let contract = RecordStoreContract::definition()?.capability;
        let requirements: Vec<_> = schema.requires
            .iter()
            .filter(|requirement| requirement.capability == contract)
            .collect();
        let requirement = match requirements.as_slice() {
            [] => return Err(SdkError::MissingStorage),
            [only] => only.name.as_str(),
            _ => return Err(SdkError::AmbiguousStorage),
        };
        let store = self.running
            .service::<Arc<dyn RecordStore + Send + Sync>>(requirement)?
            .clone();
        Ok(Records::from_store(store))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RieselModel;
    use serde::Deserialize;

    #[derive(Debug, Serialize, Deserialize, RieselModel)]
    #[riesel(model = "todo")]
    struct TodoModel {
        title: String,
        completed: bool,
    }

    #[cfg(feature = "dev-memory-storage")]
    #[test]
    fn rust_first_app_runs_without_json_or_tcp() {
        Application::new("Todo")
            .records::<TodoModel>()
            .unwrap()
            .development()
            .unwrap()
            .run(|ctx| {
                let records = ctx.records::<TodoModel>()?;
                let created = records.create(TodoModel {
                    title: "Build Riesel".into(),
                    completed: false,
                })?;
                assert_eq!(created.revision, 1);
                let got = records.get(created.id)?.unwrap();
                assert_eq!(got.value.title, "Build Riesel");
                assert_eq!(records.list()?.len(), 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn production_does_not_silently_fall_back_to_memory() {
        let result = Application::new("Todo")
            .records::<TodoModel>()
            .unwrap()
            .run(|_| Ok(()));
        assert!(matches!(
            result,
            Err(SdkError::Execution(AppRunError::Startup(
                RuntimeError::NoLocalProvider { .. }
            )))
        ));
    }
}
