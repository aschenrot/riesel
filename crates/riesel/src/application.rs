use crate::{CancellationToken, Records, RieselModel, RpcConnector};
use riesel_app::{
    App, AppRunError, ConnectorError, ConnectorRegistry, DeploymentConfig, DeploymentPlan,
    LocalProviderFactory, RunningApp, RuntimeError,
};
use riesel_protocol::RpcClientBinding;
use riesel_schema::{AppDefinition, AppSchema, CapabilityDescriptor, CapabilityRequirement};
use riesel_storage::RecordStoreContract;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use std::path::PathBuf;
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
    #[error(transparent)]
    Schema(#[from] riesel_schema::SchemaError),
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
    #[error("capability requirement not found: {0}")]
    MissingCapability(String),
    #[error("capability requirement {0} does not match the requested contract")]
    CapabilityMismatch(String),
    #[error("requirement {0} is already used for a different capability")]
    RequirementConflict(String),
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

    /// Declare an arbitrary generated Rust capability without copying its
    /// identifier or version into JSON or application logic.
    pub fn require<C: CapabilityDescriptor>(mut self, name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let capability = C::definition()?.capability;
        if let Some(previous) = self
            .definition
            .requires
            .iter()
            .find(|item| item.name == name)
        {
            if previous.capability != capability {
                return Err(SdkError::RequirementConflict(name));
            }
        } else {
            self.definition.requires.push(CapabilityRequirement {
                name,
                capability,
                depends_on: Vec::new(),
            });
        }
        Ok(self)
    }

    /// Declare startup dependencies by requirement alias. This expresses
    /// lifecycle order, not a new capability contract or provider dependency.
    pub fn depends_on<I, S>(mut self, name: &str, dependencies: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let requirement = self
            .definition
            .requires
            .iter_mut()
            .find(|requirement| requirement.name == name)
            .ok_or_else(|| SdkError::MissingCapability(name.into()))?;
        requirement.depends_on = dependencies.into_iter().map(Into::into).collect();
        Ok(self)
    }

    /// Declare a generated RPC capability and register its transport adapter
    /// once, even when several requirements share the same capability.
    pub fn rpc<C: RpcClientBinding + 'static>(mut self, name: impl Into<String>) -> Result<Self> {
        let capability = C::definition()?.capability;
        self = self.require::<C>(name)?;
        if !self.providers.has_connector(&capability) {
            self.providers.register(RpcConnector::<C>::new())?;
        }
        Ok(self)
    }

    /// Register a custom capability transport adapter.
    pub fn connector(
        mut self,
        connector: impl riesel_app::CapabilityConnector + 'static,
    ) -> Result<Self> {
        self.providers.register(connector)?;
        Ok(self)
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

        self.rpc::<RecordStoreContract>("storage")
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

    /// Export the canonical application schema generated from Rust types.
    /// This is suitable for tools and visual builders; it is not a second
    /// authoritative file that a Rust developer has to edit.
    pub fn definition_json(&self) -> Result<String> {
        self.definition.validate()?;
        Ok(serde_json::to_string_pretty(&self.definition)?)
    }

    /// Inspect selected providers and dependency order without starting a
    /// service, opening a socket, or invoking application business logic.
    pub fn deployment_plan(&self, config: &DeploymentConfig) -> Result<DeploymentPlan> {
        let app = App::from_definition(self.definition.clone())?;
        Ok(app.preflight(config, &self.providers)?)
    }

    pub fn run(
        self,
        mut entrypoint: impl FnMut(&Context<'_>) -> std::result::Result<(), ConnectorError>,
    ) -> Result<()> {
        let app = App::from_definition(self.definition)?;
        let invoke = |running: &RunningApp| entrypoint(&Context { running });
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
        entrypoint: impl FnMut(&Context<'_>) -> std::result::Result<(), ConnectorError>,
    ) -> Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        match args.as_slice() {
            [] => {}
            [option] if option == "--help" || option == "-h" => {
                println!(
                    "Usage: <application> [--deployment FILE | --inspect-deployment FILE | --print-definition]"
                );
                return Ok(());
            }
            [option] if option == "--print-definition" => {
                println!("{}", self.definition_json()?);
                return Ok(());
            }
            [option, path] if option == "--deployment" => {
                self.deployment = Some(DeploymentConfig::load(PathBuf::from(path))?);
            }
            [option, path] if option == "--inspect-deployment" => {
                let config = DeploymentConfig::load(PathBuf::from(path))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&self.deployment_plan(&config)?)?
                );
                return Ok(());
            }
            _ => {
                return Err(SdkError::Arguments(
                    "expected --deployment FILE, --inspect-deployment FILE, --print-definition or --help".into(),
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
    /// Cooperative cancellation triggered by an unexpected managed-service
    /// exit (or application shutdown). User code must observe this token.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.running.cancellation_token()
    }

    /// Start work owned by this application. On service failure or shutdown
    /// the task's cancellation token is signalled; all tasks must finish
    /// before managed providers can be torn down or restarted.
    pub fn spawn_task(
        &self,
        name: impl Into<String>,
        task: impl FnOnce(CancellationToken) -> std::result::Result<(), ConnectorError> + Send + 'static,
    ) -> Result<()> {
        Ok(self.running.spawn_task(name, task)?)
    }

    /// Retrieve a typed capability service generated from a Rust contract.
    /// The requirement's declared version is checked before downcasting.
    pub fn capability<C: RpcClientBinding>(&self, name: &str) -> Result<&C::Service> {
        let definition = self
            .running
            .app()
            .definition()
            .ok_or_else(|| SdkError::MissingCapability(name.into()))?;
        let expected = C::definition()?.capability;
        let requirement = definition
            .requires
            .iter()
            .find(|requirement| requirement.name == name)
            .ok_or_else(|| SdkError::MissingCapability(name.into()))?;
        if requirement.capability != expected {
            return Err(SdkError::CapabilityMismatch(name.into()));
        }
        Ok(self.running.service::<C::Service>(name)?)
    }

    pub fn records<T>(&self) -> Result<Records<T>>
    where
        T: RieselModel + Serialize + DeserializeOwned,
    {
        let schema = self
            .running
            .app()
            .definition()
            .ok_or(SdkError::MissingStorage)?;
        let registered = schema
            .models
            .get(T::MODEL_NAME)
            .ok_or_else(|| SdkError::MissingModel(T::MODEL_NAME.into()))?;
        if registered != &T::model_definition() {
            return Err(SdkError::ModelMismatch(T::MODEL_NAME.into()));
        }

        let contract = RecordStoreContract::definition()?.capability;
        let requirements: Vec<_> = schema
            .requires
            .iter()
            .filter(|requirement| requirement.capability == contract)
            .collect();
        let requirement = match requirements.as_slice() {
            [] => return Err(SdkError::MissingStorage),
            [only] => only.name.as_str(),
            _ => return Err(SdkError::AmbiguousStorage),
        };
        let store = self.capability::<RecordStoreContract>(requirement)?.clone();
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
                let storage = ctx.capability::<RecordStoreContract>("storage")?;
                assert_eq!(storage.list("todo")?.len(), 1);
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
    #[test]
    fn sdk_generated_definition_round_trips_without_handwritten_json() {
        let app = Application::new("Todo").records::<TodoModel>().unwrap();
        let json = app.definition_json().unwrap();
        let decoded = riesel_schema::parse_schema(&json).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.app.name, "Todo");
        assert_eq!(decoded.models["todo"], TodoModel::model_definition());
        assert_eq!(decoded.requires[0].capability.id, "riesel.storage.records");
    }
    #[test]
    fn generated_rpc_capability_is_registered_once_for_multiple_aliases() {
        let app = Application::new("Example")
            .rpc::<RecordStoreContract>("primary")
            .unwrap()
            .rpc::<RecordStoreContract>("secondary")
            .unwrap();
        assert_eq!(app.definition().requires.len(), 2);
        assert!(app.providers.has_connector("riesel.storage.records"));
    }

    #[test]
    fn arbitrary_rust_capability_is_declared_by_its_generated_type() {
        let app = Application::new("Example")
            .require::<RecordStoreContract>("records")
            .unwrap();
        assert_eq!(app.definition().requires[0].name, "records");
        assert_eq!(
            app.definition().requires[0].capability,
            RecordStoreContract::definition().unwrap().capability
        );
    }
    #[test]
    fn rust_sdk_exports_validated_startup_dependencies() {
        let app = Application::new("Example")
            .rpc::<RecordStoreContract>("consumer")
            .unwrap()
            .rpc::<RecordStoreContract>("storage")
            .unwrap()
            .depends_on("consumer", ["storage"])
            .unwrap();
        let generated = app.definition_json().unwrap();
        let schema = riesel_schema::parse_schema(&generated).unwrap();
        let order: Vec<_> = schema
            .startup_order()
            .unwrap()
            .into_iter()
            .map(|req| req.name.as_str())
            .collect();
        assert_eq!(order, ["storage", "consumer"]);
        assert!(matches!(
            Application::new("Example")
                .rpc::<RecordStoreContract>("consumer")
                .unwrap()
                .depends_on("consumer", ["missing"])
                .unwrap()
                .definition_json(),
            Err(SdkError::Schema(
                riesel_schema::SchemaError::UnknownDependency { .. }
            ))
        ));
    }

    #[test]
    fn rust_sdk_inspects_deployment_without_starting_configured_executable() {
        let app = Application::new("Example")
            .rpc::<RecordStoreContract>("consumer")
            .unwrap()
            .rpc::<RecordStoreContract>("storage")
            .unwrap()
            .depends_on("consumer", ["storage"])
            .unwrap();
        let config = DeploymentConfig::from_json(
            r#"{"services":{
                "consumer":{"transport":"tcp","address":"127.0.0.1:41911"},
                "storage":{"transport":"managed","program":"not-a-real-service"}
            }}"#,
        )
        .unwrap();
        let plan = app.deployment_plan(&config).unwrap();
        let ordered: Vec<_> = plan
            .services
            .iter()
            .map(|service| service.requirement.as_str())
            .collect();
        assert_eq!(ordered, ["storage", "consumer"]);
        assert!(
            serde_json::to_string(&plan)
                .unwrap()
                .contains("not-a-real-service")
        );
    }
}
