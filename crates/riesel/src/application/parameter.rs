//! Typed, owned parameters for Rust-first application authoring.
//! Registration and extraction share one contract; high-level macros reuse
//! these mechanisms instead of duplicating requirement and context wiring.

use super::{Application, Context, Result};
#[cfg(feature = "async-tcp")]
use riesel_protocol::AsyncRpcClientBinding;
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
/// This wrapper is synchronous; native async contracts use `AsyncCapability`.
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

/// Typed native Record Query result decoding, without adding a Storage
/// mutation requirement or doing consumer-side filtering.
#[cfg(feature = "records")]
impl<T> ModelRead<T, riesel_storage::RecordQueryContract>
where
    T: RieselModel + DeserializeOwned,
{
    pub fn page(
        &self,
        predicate: Option<riesel_storage::RecordPredicate>,
        limit: u64,
        scan_budget: u64,
        cursor: Option<String>,
    ) -> Result<crate::QueryPage<T>> {
        use riesel_storage::RecordQuery;

        let result = self.capability.service().query_page(riesel_storage::RecordQueryRequest {
            model: T::MODEL_NAME.into(),
            predicate,
            limit,
            scan_budget,
            cursor,
        })?;
        let records = result
            .records
            .into_iter()
            .map(crate::records::decode_record::<T>)
            .collect::<Result<Vec<_>>>()?;
        Ok(crate::QueryPage {
            records,
            next_cursor: result.next_cursor,
            exhausted: result.exhausted,
            inspected: result.inspected,
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

/// A typed, owned async remote capability handle. This is injected through
/// the existing dependency resolver and does not start its own executor.
#[cfg(feature = "async-tcp")]
pub struct AsyncCapability<C: AsyncRpcClientBinding> {
    service: C::Service,
    _contract: PhantomData<fn() -> C>,
}

#[cfg(feature = "async-tcp")]
impl<C: AsyncRpcClientBinding> std::ops::Deref for AsyncCapability<C> {
    type Target = C::Service;

    fn deref(&self) -> &Self::Target {
        &self.service
    }
}

#[cfg(feature = "async-tcp")]
impl<C: AsyncRpcClientBinding> Clone for AsyncCapability<C>
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

#[cfg(feature = "async-tcp")]
impl<C> ApplicationParam for AsyncCapability<C>
where
    C: AsyncRpcClientBinding + 'static,
    C::Service: Clone,
{
    fn default_alias() -> Result<String> {
        Ok(C::definition()?.capability.default_requirement_alias())
    }

    fn declare(application: Application, alias: &str) -> Result<Application> {
        application.async_rpc::<C>(alias)
    }

    fn extract(context: &Context<'_>, alias: &str) -> Result<Self> {
        Ok(Self {
            service: context.async_capability::<C>(alias)?.clone(),
            _contract: PhantomData,
        })
    }
}

/// A typed, zero-cost injection adapter for generated native async handles.
/// It avoids overlapping blanket ApplicationParam implementations with
/// existing parameter types (notably the session-scoped AsyncTasks).
#[cfg(feature = "async-tcp")]
#[doc(hidden)]
pub struct NativeHandleParam<H: riesel_protocol::NativeAsyncHandle>(H);

#[cfg(feature = "async-tcp")]
impl<H: riesel_protocol::NativeAsyncHandle> NativeHandleParam<H> {
    pub fn into_inner(self) -> H {
        self.0
    }
}

#[cfg(feature = "async-tcp")]
impl<H: riesel_protocol::NativeAsyncHandle> ApplicationParam for NativeHandleParam<H> {
    fn default_alias() -> Result<String> {
        Ok(
            <H::Contract as riesel_schema::CapabilityDescriptor>::definition()?
                .capability
                .default_requirement_alias(),
        )
    }

    fn declare(application: Application, alias: &str) -> Result<Application> {
        application.async_rpc::<H::Contract>(alias)
    }

    fn extract(context: &Context<'_>, alias: &str) -> Result<Self> {
        Ok(Self(
            context.async_capability::<H::Contract>(alias)?.clone(),
        ))
    }
}

/// Injection of session-owned async background work without introducing a
/// capability dependency, provider, or transport into the app schema.
/// The spawner is valid only inside its owning async handler invocation.
#[cfg(feature = "async-tcp")]
impl ApplicationParam for crate::AsyncTasks {
    fn default_alias() -> Result<String> {
        Ok("tasks".into())
    }

    fn declare(application: Application, _alias: &str) -> Result<Application> {
        Ok(application)
    }

    fn extract(context: &Context<'_>, _alias: &str) -> Result<Self> {
        Ok(context.async_tasks())
    }
}
