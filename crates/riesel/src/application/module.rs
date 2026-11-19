//! Optional grouping of ordinary Rust application registrations.
//! Modules neither introduce service state nor define another runtime.
use super::{Application, Result, SdkError};

/// A reusable group of application requirements, components and
/// dependency declarations. Every operation uses the existing builder,
/// one authoritative AppSchema and one runtime provider registry.
pub trait ApplicationModule: 'static {
    fn register(application: Application) -> Result<Application>;
}

impl Application {
    /// Compose one module. The module type path is its private identity:
    /// no manually managed ID is necessary for code-local composition.
    /// Capability requirements still determine runtime startup order;
    /// this registration order creates no separate lifecycle graph.
    pub fn module<M: ApplicationModule>(mut self) -> Result<Self> {
        let name = std::any::type_name::<M>().to_owned();
        if !self.modules.insert(name.clone()) {
            return Err(SdkError::DuplicateModule(name));
        }
        let composed = M::register(self)?;
        if !composed.modules.contains(&name) {
            return Err(SdkError::ModuleReplacedApplication(name));
        }
        Ok(composed)
    }
}
