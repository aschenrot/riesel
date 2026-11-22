use super::*;

/// Application-level service access. Callers never downcast or choose
/// transport-specific trait object types.
pub struct Context<'a> {
    pub(super) running: &'a RunningApp,
}

impl Context<'_> {
    /// Cooperative cancellation triggered by an unexpected managed-service
    /// exit (or application shutdown). User code must observe this token.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.running.cancellation_token()
    }

    /// Task spawner scoped to this asynchronous application session.
    /// It cannot admit tasks outside the active async entrypoint.
    #[cfg(feature = "async-tcp")]
    pub fn async_tasks(&self) -> crate::AsyncTasks {
        self.running.async_tasks()
    }

    #[cfg(feature = "async-tcp")]
    pub(crate) fn session_token(&self) -> riesel_protocol::SessionToken {
        self.running.session_token()
    }

    /// Internal #157 fixture bridge to the *actual* Riesel session generation.
    /// Not a shipped application export interface or client permission token.
    #[cfg(feature = "internal-publication")]
    #[doc(hidden)]
    pub fn internal_publication_session(
        &self,
    ) -> riesel_protocol::session::InternalPublicationSession {
        self.running.session_token().internal_publication_session()
    }

    /// Internal #158 evidence snapshot issued by the Riesel runtime.
    /// This is not a stable SDK observer, a published interface, or a grant.
    #[cfg(feature = "internal-publication")]
    #[doc(hidden)]
    pub fn internal_operational_observations(
        &self,
    ) -> std::result::Result<
        riesel_app::operational_observation::ObservationSnapshot,
        riesel_app::operational_observation::ObservationError,
    > {
        self.running.internal_operational_observations()
    }

    /// Internal, test-feature-only SDK lexical entrypoint marker. The runtime
    /// records this phase independently of provider startup or module hooks.
    #[cfg(feature = "internal-publication")]
    pub(crate) fn internal_enter_entrypoint(
        &self,
    ) -> std::io::Result<riesel_app::InternalEntrypointGuard> {
        self.running.async_tasks().internal_enter_entrypoint()
    }

    /// Construct a bounded graph runner in this application's actual session.
    /// Operations remain the application's own Rust bindings; no provider
    /// resolution or independent executor is performed.
    #[cfg(feature = "async-tcp")]
    pub fn execution_graph(
        &self,
        plan: crate::BoundExecutionPlan<crate::AsyncOperation>,
        max_in_flight: std::num::NonZeroUsize,
    ) -> std::result::Result<crate::ExecutionGraphRunner, crate::GraphRunError> {
        self.running.execution_graph(plan, max_in_flight)
    }

    /// Run a separately validated, opt-in typed graph on the same session
    /// admission owner as the existing S2 graph. No additional scheduler.
    #[cfg(feature = "experimental-typed-dataflow")]
    pub fn typed_execution_graph(
        &self,
        prepared: crate::PreparedTypedExecution,
        max_in_flight: std::num::NonZeroUsize,
    ) -> std::result::Result<crate::TypedExecutionGraphRunner, crate::GraphRunError> {
        self.running.typed_execution_graph(prepared, max_in_flight)
    }

    /// Start work owned by this application. On service failure or shutdown
    /// the task's cancellation token is signalled; all tasks must finish
    /// before managed providers can be torn down or restarted.
    pub fn spawn_task(
        &self,
        name: impl Into<String>,
        task: impl FnOnce(CancellationToken) -> std::result::Result<(), ConnectorError> + Send + 'static,
    ) -> Result<()> {
        Ok(self.running.spawn_task(name, task)?)
    }

    /// Retrieve a typed capability service generated from a Rust contract.
    /// The requirement's declared version is checked before downcasting.
    pub fn capability<C: RpcClientBinding>(&self, name: &str) -> Result<&C::Service> {
        self.checked_service::<C, C::Service>(name)
    }

    #[cfg(feature = "async-tcp")]
    pub fn async_capability<C: AsyncRpcClientBinding>(&self, name: &str) -> Result<&C::Service> {
        self.checked_service::<C, C::Service>(name)
    }

    fn checked_service<C: CapabilityDescriptor, T: std::any::Any + Send + Sync>(
        &self,
        name: &str,
    ) -> Result<&T> {
        let definition = self.running.app().definition();
        let expected = C::definition()?.capability;
        let requirement = definition
            .requires
            .iter()
            .find(|requirement| requirement.name == name)
            .ok_or_else(|| SdkError::MissingCapability(name.into()))?;
        if requirement.capability != expected {
            return Err(SdkError::CapabilityMismatch(name.into()));
        }
        Ok(self.running.service::<T>(name)?)
    }

    #[cfg(feature = "records")]
    pub fn records<T>(&self) -> Result<Records<T>>
    where
        T: RieselModel + Serialize + DeserializeOwned,
    {
        self.validate_model::<T>()?;
        let schema = self.running.app().definition();
        let contract = RecordStoreContract::definition()?.capability;
        let requirement = if let Some(bindings) = schema.model_bindings.get(T::MODEL_NAME) {
            match bindings.as_slice() {
                [] => return Err(SdkError::MissingStorage),
                [only] => only.as_str(),
                _ => return Err(SdkError::AmbiguousStorage),
            }
        } else {
            // External schemas predating explicit model bindings can still
            // resolve the single unambiguous record-storage requirement.
            let requirements: Vec<_> = schema
                .requires
                .iter()
                .filter(|requirement| requirement.capability == contract)
                .collect();
            match requirements.as_slice() {
                [] => return Err(SdkError::MissingStorage),
                [only] => only.name.as_str(),
                _ => return Err(SdkError::AmbiguousStorage),
            }
        };
        self.records_at::<T>(requirement)
    }

    /// Bind a typed record handle to a named Storage requirement. This keeps
    /// multiple storage services explicit instead of inferring one alias.
    #[cfg(feature = "records")]
    pub fn records_at<T>(&self, requirement: &str) -> Result<Records<T>>
    where
        T: RieselModel + Serialize + DeserializeOwned,
    {
        self.validate_model::<T>()?;
        let store = self.capability::<RecordStoreContract>(requirement)?.clone();
        Ok(Records::from_store(store))
    }

    #[cfg(feature = "records")]
    fn validate_model<T: RieselModel>(&self) -> Result<()> {
        let schema = self.running.app().definition();
        let registered = schema
            .models
            .get(T::MODEL_NAME)
            .ok_or_else(|| SdkError::MissingModel(T::MODEL_NAME.into()))?;
        if registered != &T::model_definition() {
            return Err(SdkError::ModelMismatch(T::MODEL_NAME.into()));
        }
        Ok(())
    }
}
