use crate::ResolvedCapability;
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BindingError {
    #[error("provider instance already bound: {0}")]
    DuplicateProvider(String),
    #[error("no instance of provider {provider} bound for requirement {requirement}")]
    MissingInstance {
        requirement: String,
        provider: String,
    },
}

/// A typed, in-process service table used after capability resolution.
/// T is a Rust capability trait object (e.g. dyn RecordStore).
/// Metadata resolution is separate from constructing provider instances.
pub struct CapabilityBindings<T: ?Sized> {
    instances: BTreeMap<String, Arc<T>>,
}

impl<T: ?Sized> Default for CapabilityBindings<T> {
    fn default() -> Self {
        Self {
            instances: BTreeMap::new(),
        }
    }
}

impl<T: ?Sized> CapabilityBindings<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a constructed service under its declared provider ID.
    pub fn register(
        &mut self,
        provider: impl Into<String>,
        instance: Arc<T>,
    ) -> Result<(), BindingError> {
        let provider = provider.into();
        if self.instances.contains_key(&provider) {
            return Err(BindingError::DuplicateProvider(provider));
        }
        self.instances.insert(provider, instance);
        Ok(())
    }

    /// Retrieves the instance selected by the versioned metadata resolver.
    /// A shared Arc allows multiple consumers to use the same provider.
    pub fn get(&self, resolved: &ResolvedCapability) -> Result<Arc<T>, BindingError> {
        self.instances
            .get(&resolved.provider)
            .cloned()
            .ok_or_else(|| BindingError::MissingInstance {
                requirement: resolved.requirement.clone(),
                provider: resolved.provider.clone(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_schema::CapabilityRef;

    trait Greeter {
        fn message(&self) -> &'static str;
    }

    struct Hello;

    impl Greeter for Hello {
        fn message(&self) -> &'static str {
            "Hello"
        }
    }

    fn resolved() -> ResolvedCapability {
        ResolvedCapability {
            requirement: "greeter".into(),
            capability: CapabilityRef {
                id: "example.greeter".into(),
                version: 1,
            },
            provider: "example.greeter.local".into(),
        }
    }

    #[test]
    fn injects_an_implementation_of_a_typed_capability() {
        let mut bindings = CapabilityBindings::<dyn Greeter>::new();
        bindings
            .register("example.greeter.local", Arc::new(Hello))
            .unwrap();

        assert_eq!(bindings.get(&resolved()).unwrap().message(), "Hello");
        assert!(matches!(
            bindings.register("example.greeter.local", Arc::new(Hello)),
            Err(BindingError::DuplicateProvider(_))
        ));
    }

    #[test]
    fn missing_instance_is_reported_instead_of_silently_created() {
        let bindings = CapabilityBindings::<dyn Greeter>::new();
        assert!(matches!(
            bindings.get(&resolved()),
            Err(BindingError::MissingInstance { .. })
        ));
    }
}
