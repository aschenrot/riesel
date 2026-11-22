mod capability;
mod capability_contract;
mod dependency_order;
mod model;
mod model_schema;
mod provider;
mod record_validation;
mod schema;

pub use capability::{CapabilityRef, CapabilityRefError};
pub use capability_contract::{
    CapabilityContract, CapabilityDescriptor, ContractError, OperationDefinition, OperationKind,
    ParameterDefinition, RieselType, TypeDefinition, TypeRef,
};
pub use model::{ModelField, RieselModel};
pub use model_schema::{FieldType, ModelDefinition, RecordError};
pub use provider::{ProviderDefinition, ProviderError};

pub use schema::{
    AppDefinition, AppSchema, CapabilityRequirement, LinkedRecordConsistency, LinkedRecordQuery,
    SchemaError, parse_schema,
};
