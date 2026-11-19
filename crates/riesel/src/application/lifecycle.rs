//! Optional named hooks participate in the existing application session.
//! Modules register hooks through Application; they do not own executors,
//! provider instances or a separate lifecycle graph.
use super::*;
use riesel_app::{HookFailure, HookPhase, LifecycleFailure};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

type HookFuture<'a> =
    Pin<Box<dyn Future<Output = std::result::Result<(), ConnectorError>> + 'a>>;

trait HookHandler {
    fn invoke<'a>(&'a mut self, context: &'a Context<'_>) -> HookFuture<'a>;
}

struct TypedHook<H, Args> {
    handler: H,
    aliases: Vec<String>,
    _params: PhantomData<fn() -> Args>,
}

impl<H, Args> HookHandler for TypedHook<H, Args>
where
    Args: HandlerParams,
    H: AsyncApplicationHandler<Args>,
{
    fn invoke<'a>(&'a mut self, context: &'a Context<'_>) -> HookFuture<'a> {
        self.handler.invoke(context, &self.aliases)
    }
}

struct Hook {
    name: String,
    aliases: Vec<String>,
    handler: Box<dyn HookHandler>,
}

/// Hooks are code-local registrations, not serialized executable schema data.
#[derive(Default)]
pub(super) struct LifecycleHooks {
    start: Vec<Hook>,
    stop: Vec<Hook>,
}

impl LifecycleHooks {
    pub(super) fn is_empty(&self) -> bool {
        self.start.is_empty() && self.stop.is_empty()
    }

    fn register<Args, H>(
        &mut self,
        phase: HookPhase,
        name: String,
        aliases: Vec<String>,
        handler: H,
    ) -> Result<()>
    where
        Args: HandlerParams + 'static,
        H: AsyncApplicationHandler<Args> + 'static,
    {
        if name.is_empty()
            || name.len() > 128
            || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err(SdkError::InvalidLifecycleHookName(name));
        }
        let hooks = match phase {
            HookPhase::Start => &mut self.start,
            HookPhase::Stop => &mut self.stop,
        };
        if hooks.iter().any(|existing| existing.name == name) {
            return Err(SdkError::DuplicateLifecycleHook {
                phase: match phase {
                    HookPhase::Start => "start",
                    HookPhase::Stop => "stop",
                },
                name,
            });
        }
        hooks.push(Hook {
            name,
            aliases: aliases.clone(),
            handler: Box::new(TypedHook::<H, Args> {
                handler,
                aliases,
                _params: PhantomData,
            }),
        });
        Ok(())
    }

    /// Stable ordering uses only the capability dependency DAG. Independent
    /// hooks retain their declaration order; stop hooks run in reverse order.
    pub(super) fn order(&mut self, schema: &AppSchema) -> Result<()> {
        let order: BTreeMap<&str, usize> = schema
            .startup_order()?
            .into_iter()
            .enumerate()
            .map(|(index, requirement)| (requirement.name.as_str(), index))
            .collect();
        let rank = |hook: &Hook| {
            hook.aliases
                .iter()
                .filter_map(|alias| order.get(alias.as_str()))
                .copied()
                .max()
                .unwrap_or(0)
        };
        self.start.sort_by_key(&rank);
        self.stop.sort_by_key(rank);
        Ok(())
    }

    pub(super) async fn execute<F, Fut>(
        &mut self,
        context: &Context<'_>,
        entrypoint: F,
    ) -> std::result::Result<(), ConnectorError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = std::result::Result<(), ConnectorError>>,
    {
        let mut initialized = BTreeSet::new();
        let mut failures = Vec::new();
        for hook in &mut self.start {
            match hook.handler.invoke(context).await {
                Ok(()) => {
                    initialized.insert(hook.name.clone());
                }
                Err(source) => {
                    failures.push(HookFailure {
                        phase: HookPhase::Start,
                        name: hook.name.clone(),
                        source,
                    });
                    break;
                }
            }
        }
        let initialized_all = failures.is_empty();
        let entry = if initialized_all {
            entrypoint().await.err()
        } else {
            None
        };

        // Background work cannot race the module's stop hooks or extend
        // the provider's lifetime. Session handles remain valid here.
        let tasks = context.async_tasks();
        let task_shutdown = tasks.prepare_stop().await.err();
        if context.session_token().is_active() {
            for hook in self.stop.iter_mut().rev() {
                if initialized_all || initialized.contains(&hook.name) {
                    if let Err(source) = hook.handler.invoke(context).await {
                        failures.push(HookFailure {
                            phase: HookPhase::Stop,
                            name: hook.name.clone(),
                            source,
                        });
                    }
                }
            }
        }

        if !failures.is_empty() {
            Err(Box::new(LifecycleFailure {
                entry,
                hooks: failures,
            }))
        } else if let Some(entry) = entry {
            Err(entry)
        } else if let Some(error) = task_shutdown {
            Err(Box::new(error))
        } else {
            Ok(())
        }
    }
}

impl Application {
    /// Add one optional asynchronous startup hook. A matching stop hook can
    /// use the same name. Hooks use the active session's typed dependencies.
    pub fn on_start<Args, H>(self, name: impl Into<String>, handler: H) -> Result<Self>
    where
        Args: HandlerParams + 'static,
        H: AsyncApplicationHandler<Args> + 'static,
    {
        self.on_start_with_aliases(name, handler, Args::default_aliases()?)
    }

    /// Explicit requirement aliases for a startup hook.
    pub fn on_start_with_aliases<Args, H, I, S>(
        self,
        name: impl Into<String>,
        handler: H,
        aliases: I,
    ) -> Result<Self>
    where
        Args: HandlerParams + 'static,
        H: AsyncApplicationHandler<Args> + 'static,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let aliases: Vec<String> = aliases.into_iter().map(Into::into).collect();
        let mut application = Args::declare(self, &aliases)?;
        application
            .lifecycle
            .register::<Args, H>(HookPhase::Start, name.into(), aliases, handler)?;
        Ok(application)
    }

    /// Add one optional asynchronous shutdown hook. It runs while providers
    /// remain alive, after owned async background work is drained.
    pub fn on_stop<Args, H>(self, name: impl Into<String>, handler: H) -> Result<Self>
    where
        Args: HandlerParams + 'static,
        H: AsyncApplicationHandler<Args> + 'static,
    {
        self.on_stop_with_aliases(name, handler, Args::default_aliases()?)
    }

    /// Explicit requirement aliases for a shutdown hook.
    pub fn on_stop_with_aliases<Args, H, I, S>(
        self,
        name: impl Into<String>,
        handler: H,
        aliases: I,
    ) -> Result<Self>
    where
        Args: HandlerParams + 'static,
        H: AsyncApplicationHandler<Args> + 'static,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let aliases: Vec<String> = aliases.into_iter().map(Into::into).collect();
        let mut application = Args::declare(self, &aliases)?;
        application
            .lifecycle
            .register::<Args, H>(HookPhase::Stop, name.into(), aliases, handler)?;
        Ok(application)
    }
}
