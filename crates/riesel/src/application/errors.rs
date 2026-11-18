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
