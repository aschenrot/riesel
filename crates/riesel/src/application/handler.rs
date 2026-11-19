//! Function-signature-driven synchronous handler registration.
//! The future high-level application macro builds on these semantics.
//! A handler is invoked afresh for each runtime session (including recovery).

use super::{
    Application, ApplicationParam, Context, DeploymentConfig, DeploymentPlan, Result, SdkError,
};
use riesel_app::{ConnectorError, LocalProviderFactory};
use riesel_schema::AppSchema;
use std::any::TypeId;
use std::collections::HashSet;
use std::marker::PhantomData;

/// Parameter tuples derive their requirements from the Rust function types.
/// Different Records<T> models may intentionally share one Storage alias.
pub trait HandlerParams: Sized {
    fn default_aliases() -> Result<Vec<String>>;
    fn declare(application: Application, aliases: &[String]) -> Result<Application>;
    fn extract(context: &Context<'_>, aliases: &[String]) -> Result<Self>;
}

impl HandlerParams for () {
    fn default_aliases() -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn declare(application: Application, aliases: &[String]) -> Result<Application> {
        if !aliases.is_empty() {
            return Err(SdkError::ParameterCount {
                expected: 0,
                actual: aliases.len(),
            });
        }
        application.definition.validate()?;
        Ok(application)
    }

    fn extract(_: &Context<'_>, _: &[String]) -> Result<Self> {
        Ok(())
    }
}

macro_rules! impl_params {
    ($($param:ident : $idx:tt),+) => {
        impl<$($param: ApplicationParam + 'static),+> HandlerParams for ($($param,)+) {
            fn default_aliases() -> Result<Vec<String>> {
                Ok(vec![$($param::default_alias()?),+])
            }

            fn declare(mut application: Application, aliases: &[String]) -> Result<Application> {
                const COUNT: usize = [$(stringify!($param)),+].len();
                if aliases.len() != COUNT {
                    return Err(SdkError::ParameterCount {
                        expected: COUNT,
                        actual: aliases.len(),
                    });
                }

                // The same typed parameter cannot accidentally resolve to
                // the same binding twice. Distinct record models, however,
                // may share a single Storage capability and provider.
                let mut seen: HashSet<(TypeId, String)> = HashSet::new();
                $(
                    if !seen.insert((TypeId::of::<$param>(), aliases[$idx].clone())) {
                        return Err(SdkError::DuplicateParameterBinding(aliases[$idx].clone()));
                    }
                    application = $param::declare(application, &aliases[$idx])?;
                )+

                // Validate collisions and aliases before any provider starts.
                application.definition.validate()?;
                Ok(application)
            }

            fn extract(context: &Context<'_>, aliases: &[String]) -> Result<Self> {
                Ok(($($param::extract(context, &aliases[$idx])?,)+))
            }
        }
    };
}

impl_params!(A: 0);
impl_params!(A: 0, B: 1);
impl_params!(A: 0, B: 1, C: 2);
impl_params!(A: 0, B: 1, C: 2, D: 3);

/// Invoke an ordinary Rust function with fresh typed dependencies.
/// Errors propagate as sources; no automatic retries or conversions to text.
pub trait ApplicationHandler<Args> {
    fn invoke(
        &mut self,
        context: &Context<'_>,
        aliases: &[String],
    ) -> std::result::Result<(), ConnectorError>;
}

impl<F, E> ApplicationHandler<()> for F
where
    F: FnMut() -> std::result::Result<(), E>,
    E: Into<ConnectorError>,
{
    fn invoke(&mut self, _: &Context<'_>, _: &[String]) -> std::result::Result<(), ConnectorError> {
        self().map_err(Into::into)
    }
}

macro_rules! impl_handler {
    ($($param:ident : $binding:ident),+) => {
        impl<F, E, $($param),+> ApplicationHandler<($($param,)+)> for F
        where
            F: FnMut($($param),+) -> std::result::Result<(), E>,
            E: Into<ConnectorError>,
            $($param: ApplicationParam + 'static),+
        {
            fn invoke(
                &mut self,
                context: &Context<'_>,
                aliases: &[String],
            ) -> std::result::Result<(), ConnectorError> {
                let ($($binding,)+) =
                    <($($param,)+) as HandlerParams>::extract(context, aliases)
                        .map_err(|error| -> ConnectorError { Box::new(error) })?;
                self($($binding),+).map_err(Into::into)
            }
        }
    };
}

impl_handler!(A: a);
impl_handler!(A: a, B: b);
impl_handler!(A: a, B: b, C: c);
impl_handler!(A: a, B: b, C: c, D: d);

/// An application with a declared handler and fully validated dependencies.
/// Provider factories and deployment remain explicit and configurable.
pub struct PreparedApplication<H, Args> {
    application: Application,
    handler: H,
    aliases: Vec<String>,
    _params: PhantomData<fn() -> Args>,
}

impl Application {
    /// Declare the requirements of a Rust handler from its typed parameters.
    /// Parameter variable names never become deployment binding aliases.
    pub fn handler<Args, H>(self, handler: H) -> Result<PreparedApplication<H, Args>>
    where
        Args: HandlerParams,
        H: ApplicationHandler<Args>,
    {
        self.handler_with_aliases(handler, Args::default_aliases()?)
    }

    /// Explicitly name bindings in parameter order, for example when two
    /// independent instances of the same capability are required.
    /// The application macro will translate parameter binding attributes
    /// into this call without exposing positional aliases to developers.
    pub fn handler_with_aliases<Args, H, I, S>(
        self,
        handler: H,
        aliases: I,
    ) -> Result<PreparedApplication<H, Args>>
    where
        Args: HandlerParams,
        H: ApplicationHandler<Args>,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let aliases: Vec<String> = aliases.into_iter().map(Into::into).collect();
        let application = Args::declare(self, &aliases)?;
        Ok(PreparedApplication {
            application,
            handler,
            aliases,
            _params: PhantomData,
        })
    }
}

impl<H, Args> PreparedApplication<H, Args>
where
    Args: HandlerParams,
    H: ApplicationHandler<Args>,
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

    pub fn module<M: super::ApplicationModule>(mut self) -> Result<Self> {
        self.application = self.application.module::<M>()?;
        Ok(self)
    }

    pub fn component<C: riesel_app::DeclarativeComponent>(mut self) -> Result<Self> {
        self.application = self.application.component::<C>()?;
        Ok(self)
    }

    pub fn provider(mut self, provider: impl LocalProviderFactory + 'static) -> Result<Self> {
        self.application = self.application.provider(provider)?;
        Ok(self)
    }

    pub fn provide<C, F>(mut self, id: impl Into<String>, create: F) -> Result<Self>
    where
        C: riesel_protocol::RpcClientBinding + 'static,
        F: Fn() -> C::Service + Send + Sync + 'static,
    {
        self.application = self.application.provide::<C, F>(id, create)?;
        Ok(self)
    }

    #[cfg(feature = "dev-memory-storage")]
    pub fn development(mut self) -> Result<Self> {
        self.application = self.application.development()?;
        Ok(self)
    }

    pub fn deployment(mut self, deployment: DeploymentConfig) -> Self {
        self.application = self.application.deployment(deployment);
        self
    }

    pub fn catalog(mut self, catalog: riesel_app::ProviderCatalog) -> Self {
        self.application = self.application.catalog(catalog);
        self
    }

    pub fn select_provider(
        mut self,
        requirement: impl Into<String>,
        choice: impl Into<String>,
    ) -> Result<Self> {
        self.application = self.application.select_provider(requirement, choice)?;
        Ok(self)
    }

    pub fn catalog_plan(&self) -> Result<DeploymentPlan> {
        self.application.catalog_plan()
    }

    pub fn provider_selections(
        mut self,
        config: riesel_app::ProviderSelectionConfig,
    ) -> Result<Self> {
        self.application = self.application.provider_selections(config)?;
        Ok(self)
    }

    /// Execute a handler once per started session. Managed recovery can
    /// replay it under the existing opt-in whole-session recovery policy.
    pub fn run(self) -> Result<()> {
        let Self {
            application,
            mut handler,
            aliases,
            ..
        } = self;
        application.run(move |context| handler.invoke(context, &aliases))
    }

    pub fn run_from_env(self) -> Result<()> {
        let Self {
            application,
            mut handler,
            aliases,
            ..
        } = self;
        application.run_from_env(move |context| handler.invoke(context, &aliases))
    }
}
