//! Session-owned asynchronous entrypoints using the exact dependency
//! declaration and extraction mechanism as synchronous handler functions.
//! A private Tokio executor belongs to a runtime session, never to user code.

use super::{Application, ApplicationParam, Context, DeploymentConfig, DeploymentPlan, Result};
use crate::application::HandlerParams;
use riesel_app::{ConnectorError, LocalProviderFactory};
use riesel_protocol::{RpcError, SessionToken};
use riesel_schema::AppSchema;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Poll;

/// Run a typed async Rust function with owned per-session dependencies.
pub trait AsyncApplicationHandler<Args> {
    fn invoke<'a>(
        &'a mut self,
        context: &'a Context<'_>,
        aliases: &'a [String],
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), ConnectorError>> + 'a>>;
}

impl<F, Fut, E> AsyncApplicationHandler<()> for F
where
    F: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<(), E>>,
    E: Into<ConnectorError>,
{
    fn invoke<'a>(
        &'a mut self,
        _: &'a Context<'_>,
        _: &'a [String],
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), ConnectorError>> + 'a>> {
        Box::pin(async move { self().await.map_err(Into::into) })
    }
}

macro_rules! impl_async_handler {
    ($($param:ident : $binding:ident),+) => {
        impl<F, Fut, E, $($param),+> AsyncApplicationHandler<($($param,)+)> for F
        where
            F: FnMut($($param),+) -> Fut,
            Fut: Future<Output = std::result::Result<(), E>>,
            E: Into<ConnectorError>,
            $($param: ApplicationParam + 'static),+
        {
            fn invoke<'a>(
                &'a mut self,
                context: &'a Context<'_>,
                aliases: &'a [String],
            ) -> Pin<Box<dyn Future<Output = std::result::Result<(), ConnectorError>> + 'a>> {
                let extracted =
                    <($($param,)+) as HandlerParams>::extract(context, aliases)
                        .map_err(|error| -> ConnectorError { Box::new(error) });
                Box::pin(async move {
                    let ($($binding,)+) = extracted?;
                    self($($binding),+).await.map_err(Into::into)
                })
            }
        }
    };
}

impl_async_handler!(A: a);
impl_async_handler!(A: a, B: b);
impl_async_handler!(A: a, B: b, C: c);
impl_async_handler!(A: a, B: b, C: c, D: d);

/// Validated async entrypoint and its declared requirements. The same
/// provider registry is used for both synchronous and async applications.
pub struct PreparedAsyncApplication<H, Args> {
    application: Application,
    handler: H,
    aliases: Vec<String>,
    _params: PhantomData<fn() -> Args>,
}

impl Application {
    pub fn async_handler<Args, H>(self, handler: H) -> Result<PreparedAsyncApplication<H, Args>>
    where
        Args: HandlerParams,
        H: AsyncApplicationHandler<Args>,
    {
        self.async_handler_with_aliases(handler, Args::default_aliases()?)
    }

    pub fn async_handler_with_aliases<Args, H, I, S>(
        self,
        handler: H,
        aliases: I,
    ) -> Result<PreparedAsyncApplication<H, Args>>
    where
        Args: HandlerParams,
        H: AsyncApplicationHandler<Args>,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let aliases: Vec<_> = aliases.into_iter().map(Into::into).collect();
        let application = Args::declare(self, &aliases)?;
        Ok(PreparedAsyncApplication {
            application,
            handler,
            aliases,
            _params: PhantomData,
        })
    }
}

impl<H, Args> PreparedAsyncApplication<H, Args>
where
    Args: HandlerParams,
    H: AsyncApplicationHandler<Args>,
{
    pub fn definition(&self) -> &AppSchema {
        self.application.definition()
    }

    pub fn definition_json(&self) -> Result<String> {
        self.application.definition_json()
    }

    pub fn deployment_plan(&self, config: &DeploymentConfig) -> Result<DeploymentPlan> {
        self.application.deployment_plan(config)
    }

    pub fn component<C: riesel_app::DeclarativeComponent>(mut self) -> Result<Self> {
        self.application = self.application.component::<C>()?;
        Ok(self)
    }

    pub fn provider(mut self, provider: impl LocalProviderFactory + 'static) -> Result<Self> {
        self.application = self.application.provider(provider)?;
        Ok(self)
    }

    pub fn deployment(mut self, config: DeploymentConfig) -> Self {
        self.application = self.application.deployment(config);
        self
    }

    /// Build an executor for each application session, then execute and
    /// dispose of every scoped future before providers are torn down.
    /// Whole-session recovery constructs fresh handles on each invocation.
    pub fn run(self) -> Result<()> {
        let Self {
            application,
            mut handler,
            aliases,
            ..
        } = self;
        application.run(move |context| {
            run_scoped(
                context.session_token(),
                Some(context.async_tasks()),
                handler.invoke(context, &aliases),
            )
        })
    }

    pub fn run_from_env(self) -> Result<()> {
        let Self {
            application,
            mut handler,
            aliases,
            ..
        } = self;
        application.run_from_env(move |context| {
            run_scoped(
                context.session_token(),
                Some(context.async_tasks()),
                handler.invoke(context, &aliases),
            )
        })
    }
}

/// Every async entrypoint runs under the active application session.
/// A managed crash revokes the token and drops the pending entrypoint
/// immediately, even when it is waiting on unrelated async work.
fn run_scoped<F>(
    session: SessionToken,
    tasks: Option<crate::AsyncTasks>,
    future: F,
) -> std::result::Result<(), ConnectorError>
where
    F: Future<Output = std::result::Result<(), ConnectorError>>,
{
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if let Some(tasks) = &tasks {
        tasks.begin_scope()?;
    }
    executor.block_on(async {
        let expiry = session.cancelled();
        let mut expiry = std::pin::pin!(expiry);
        let mut future = std::pin::pin!(future);
        let outcome = std::future::poll_fn(|cx| {
            if expiry.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Err(Box::new(RpcError::SessionExpired) as ConnectorError));
            }
            match future.as_mut().poll(cx) {
                Poll::Ready(result) => {
                    if let Err(error) = session.check() {
                        Poll::Ready(Err(Box::new(error) as ConnectorError))
                    } else {
                        Poll::Ready(result)
                    }
                }
                Poll::Pending => Poll::Pending,
            }
        })
        .await;
        // Join every background future on this executor before its local
        // providers and managed services can be released by RunningApp.
        let joined = match tasks {
            Some(tasks) => tasks.finish_scope().await.map_err(|error| {
                Box::new(error) as ConnectorError
            }),
            None => Ok(()),
        };
        match (outcome, joined) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn session_crash_interrupts_pending_async_handler() {
        let session = SessionToken::new();
        let revoked = session.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            revoked.invalidate();
        });
        let error = run_scoped(
            session,
            None,
            std::future::pending::<std::result::Result<(), ConnectorError>>(),
        )
        .unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<RpcError>(),
                Some(RpcError::SessionExpired)
            ),
            "session expiry should remain a typed RPC failure"
        );
        worker.join().unwrap();
    }

    #[test]
    fn completed_async_handler_returns_before_session_shutdown() {
        let session = SessionToken::new();
        run_scoped(session, None, async { Ok(()) }).unwrap();
    }
}
