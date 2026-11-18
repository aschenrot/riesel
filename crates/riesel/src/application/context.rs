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
        Ok(self.running.service::<C::Service>(name)?)
    }

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
    pub fn records_at<T>(&self, requirement: &str) -> Result<Records<T>>
    where
        T: RieselModel + Serialize + DeserializeOwned,
    {
        self.validate_model::<T>()?;
        let store = self.capability::<RecordStoreContract>(requirement)?.clone();
        Ok(Records::from_store(store))
    }

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
