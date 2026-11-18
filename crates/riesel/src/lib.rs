//! Riesel - a general purpose application engine
pub use riesel_app::{
    App, AppConfig, AppError, ProviderRegistry, ResolutionError, ResolvedCapability,
};
pub use riesel_macros::{RieselType, capability};

pub mod prelude {
    pub use super::{App, AppConfig, AppError};
}
pub use riesel_schema as schema;
pub use riesel_storage as storage;
