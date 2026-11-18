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
        let definition = self
            .running
            .app()
            .definition()
            .ok_or_else(|| SdkError::MissingCapability(name.into()))?;
        let expected = C::definition()?.capability;
        let requirement = definition
            .requires
            .iter()
            .find(|requirement| requirement.name == name)
            .ok_or_else(|| SdkError::MissingCapability(name.into()))?;
        if requirement.capability != expected {
            return Err(SdkError::CapabilityMismatch(name.into()));
        }
        Ok(self.running.service::<C::Service>(name)?)
    }

    pub fn records<T>(&self) -> Result<Records<T>>
    where
        T: RieselModel + Serialize + DeserializeOwned,
    {
        let schema = self
            .running
            .app()
            .definition()
            .ok_or(SdkError::MissingStorage)?;
        let registered = schema
            .models
            .get(T::MODEL_NAME)
            .ok_or_else(|| SdkError::MissingModel(T::MODEL_NAME.into()))?;
        if registered != &T::model_definition() {
            return Err(SdkError::ModelMismatch(T::MODEL_NAME.into()));
        }

        let contract = RecordStoreContract::definition()?.capability;
        let requirements: Vec<_> = schema
            .requires
            .iter()
            .filter(|requirement| requirement.capability == contract)
            .collect();
        let requirement = match requirements.as_slice() {
            [] => return Err(SdkError::MissingStorage),
            [only] => only.name.as_str(),
            _ => return Err(SdkError::AmbiguousStorage),
        };
        let store = self.capability::<RecordStoreContract>(requirement)?.clone();
        Ok(Records::from_store(store))
    }
}

