use super::*;

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
    #[cfg(feature = "records")]
    Store(#[from] riesel_storage::StoreError),
    #[error(transparent)]
    RecordValidation(#[from] riesel_schema::RecordError),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Deployment(#[from] riesel_app::DeploymentError),
    #[error(transparent)]
    Catalog(#[from] riesel_app::CatalogError),
    #[error(transparent)]
    Schema(#[from] riesel_schema::SchemaError),
    #[error("model {0} has already been registered with different metadata")]
    #[cfg(feature = "records")]
    DuplicateModel(String),
    #[error("model {0} was not registered")]
    #[cfg(feature = "records")]
    MissingModel(String),
    #[error("Rust model {0} does not match the application metadata")]
    #[cfg(feature = "records")]
    ModelMismatch(String),
    #[error("record-storage capability is not declared")]
    #[cfg(feature = "records")]
    MissingStorage,
    #[error("multiple record-storage requirements are declared")]
    #[cfg(feature = "records")]
    AmbiguousStorage,
    #[error("capability requirement not found: {0}")]
    MissingCapability(String),
    #[error("capability requirement {0} does not match the requested contract")]
    CapabilityMismatch(String),
    #[error("requirement {0} is already used for a different capability")]
    RequirementConflict(String),
    #[error("capability {0} already has a different transport adapter")]
    ConnectorModeConflict(String),
    #[error("invalid application arguments: {0}")]
    Arguments(String),
    #[error("a provider catalog is required to resolve selected placements")]
    MissingProviderCatalog,
    #[error("choose either a provider catalog or an explicit deployment configuration")]
    ConflictingDeploymentSource,
    #[error("requirement {0} already has a different provider placement selection")]
    ProviderSelectionConflict(String),
    #[error("handler requires {expected} parameter aliases, but received {actual}")]
    ParameterCount { expected: usize, actual: usize },
    #[error(
        "the same handler parameter is bound more than once to {0}; use explicit distinct aliases"
    )]
    DuplicateParameterBinding(String),
}

pub type Result<T> = std::result::Result<T, SdkError>;
