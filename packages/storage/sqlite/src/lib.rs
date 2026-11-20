//! Explicit, linked-only SQLite RecordStore provider.
//! Construction is side-effect-free; the database opens in the local factory.
mod database;
mod evolution;
mod maintenance;
mod owner;
mod records;
mod restore;

pub use database::SqliteStore;
pub use evolution::{EvolutionLimits, EvolutionPreview, ModelEvolution};
pub use maintenance::{SnapshotBudget, SqliteMaintenance, VerifiedSnapshot};
pub use restore::{RestoreAuthorization, RestoreReceipt};

use riesel_app::{
    ConnectorError, LocalComponentFactory, LocalProviderFactory, LocalProviderInstance,
};
use riesel_schema::ModelDefinition;
use riesel_storage::{RecordStore, RecordStoreContract};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

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
