//! Explicit, linked-only SQLite RecordStore provider.
//! Construction is side-effect-free; the database opens in the local factory.
mod capacity;
mod database;
mod deadline;
mod evolution;
mod execute;
mod invariants;
mod maintenance;
mod operator;
mod owner;
mod plan_revisions;
mod query;
mod records;
mod restore;
mod review;

pub use database::SqliteStore;
pub use evolution::{EvolutionLimits, EvolutionPreview, ModelEvolution};
pub use execute::{EvolutionAuthorization, EvolutionReceipt};
pub use invariants::{EvolutionRecord, EvolutionRecords};
pub use maintenance::{SnapshotBudget, SqliteMaintenance, VerifiedSnapshot};
pub use operator::{OperatorOutcome, run_operator_maintenance_cli};
pub use plan_revisions::SqlitePlanRevisionRepository;
pub use restore::{RestoreAuthorization, RestoreReceipt};
pub use review::{EvolutionReview, ReviewedEvolutionAuthorization};

use riesel_app::{
    ConnectorError, LocalComponentFactory, LocalProviderFactory, LocalProviderInstance,
};
use riesel_schema::ModelDefinition;
use riesel_storage::{RecordQuery, RecordQueryContract, RecordStore, RecordStoreContract};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

/// Register a durable linked provider without touching the filesystem.
/// Models are authoritative application schema, not a second developer-maintained file.
pub fn sqlite_storage_provider(
    path: PathBuf,
    models: BTreeMap<String, ModelDefinition>,
) -> Result<impl LocalProviderFactory, ConnectorError> {
    LocalComponentFactory::new(
        "riesel.storage.sqlite",
        move |definition: &riesel_schema::ProviderDefinition| -> Result<_, ConnectorError> {
            let store: Arc<dyn RecordStore + Send + Sync> =
                Arc::new(SqliteStore::open(&path, models.clone())?);
            LocalProviderInstance::new(definition.clone()).bind::<RecordStoreContract>(store)
        },
    )
    .exposes::<RecordStoreContract>()
}

/// Register the read-designated native SQLite Query facet, optionally with
/// Storage v2 mutations exposed from the exact same live SqliteStore.
pub fn sqlite_record_query_provider(
    path: PathBuf,
    models: BTreeMap<String, ModelDefinition>,
    readable: BTreeSet<String>,
    writable: bool,
) -> Result<impl LocalProviderFactory, ConnectorError> {
    if readable.is_empty() || !readable.is_subset(&models.keys().cloned().collect()) {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "readable Query models must be registered and nonempty",
        )));
    }
    let factory = LocalComponentFactory::new(
        "riesel.storage.sqlite",
        move |definition: &riesel_schema::ProviderDefinition| -> Result<_, ConnectorError> {
            let store = Arc::new(if writable {
                SqliteStore::open(&path, models.clone())?.with_query_grants(readable.clone())?
            } else {
                SqliteStore::open_query_only(&path, models.clone(), readable.clone())?
            });
            let instance = LocalProviderInstance::new(definition.clone())
                .bind::<RecordQueryContract>(
                    Arc::clone(&store) as Arc<dyn RecordQuery + Send + Sync>
                )?;
            if writable {
                instance.bind::<RecordStoreContract>(store as Arc<dyn RecordStore + Send + Sync>)
            } else {
                Ok(instance)
            }
        },
    )
    .exposes::<RecordQueryContract>()?;
    if writable {
        factory.exposes::<RecordStoreContract>()
    } else {
        Ok(factory)
    }
}
