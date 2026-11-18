//! Typed, owned parameters for the forthcoming declarative application API.
//! Registration and extraction share one contract; high-level macros will
//! invoke this instead of duplicating requirement and context wiring.

use super::{Application, Context, Result};
use riesel_protocol::RpcClientBinding;
use std::marker::PhantomData;
use std::ops::Deref;

#[cfg(feature = "records")]
use crate::{Records, RieselModel};
#[cfg(feature = "records")]
use serde::{Serialize, de::DeserializeOwned};

/// Define both the application requirement and the resolved handler value.
/// Macros and lower-level builders use the same mechanisms.
pub trait ApplicationParam: Sized {
    /// Default identity independent of variable names in handler signatures.
    fn default_alias() -> Result<String>;

    /// Register exactly the capability/model requirements used by this value.
    fn declare(application: Application, alias: &str) -> Result<Application>;

    /// Extract an owned value from the validated, running application session.
    fn extract(context: &Context<'_>, alias: &str) -> Result<Self>;
}

/// An owned, session-scoped capability handle. Existing generated trait-Arc
/// bindings remain in use; cloning them preserves session invalidation.
/// This is synchronous until the async transport milestone is implemented.
pub struct Capability<C: RpcClientBinding> {
    service: C::Service,
    _contract: PhantomData<fn() -> C>,
}

impl<C: RpcClientBinding> Capability<C> {
    pub fn service(&self) -> &C::Service {
        &self.service
    }

    pub fn into_inner(self) -> C::Service {
        self.service
    }
}

impl<C: RpcClientBinding> Deref for Capability<C> {
    type Target = C::Service;

    fn deref(&self) -> &Self::Target {
        &self.service
    }
}

impl<C: RpcClientBinding> Clone for Capability<C>
where
    C::Service: Clone,
{
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
            _contract: PhantomData,
        }
    }
}

impl<C> ApplicationParam for Capability<C>
where
    C: RpcClientBinding + 'static,
    C::Service: Clone,
{
    fn default_alias() -> Result<String> {
        Ok(C::definition()?.capability.default_requirement_alias())
    }

    fn declare(application: Application, alias: &str) -> Result<Application> {
        application.rpc::<C>(alias)
    }

    fn extract(context: &Context<'_>, alias: &str) -> Result<Self> {
        Ok(Self {
            service: context.capability::<C>(alias)?.clone(),
            _contract: PhantomData,
        })
    }
}

#[cfg(feature = "records")]
impl<T> ApplicationParam for Records<T>
where
    T: RieselModel + Serialize + DeserializeOwned,
{
    fn default_alias() -> Result<String> {
        Ok("storage".into())
    }

    fn declare(application: Application, alias: &str) -> Result<Application> {
        application.records_at::<T>(alias)
    }

    fn extract(context: &Context<'_>, alias: &str) -> Result<Self> {
        context.records_at::<T>(alias)
    }
}
