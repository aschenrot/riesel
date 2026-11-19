use super::*;

/// Rust-first application authoring. Low-level App remains available for
/// external schemas and generated definitions.
pub struct Application {
    pub(super) definition: AppSchema,
    pub(super) providers: ConnectorRegistry,
    pub(super) deployment: Option<DeploymentConfig>,
    pub(super) catalog: Option<ProviderCatalog>,
    pub(super) selections: BTreeMap<String, String>,
    pub(super) catalog_recovery: RecoveryPolicy,
    pub(super) selection_manifest_loaded: bool,
    pub(super) modules: BTreeSet<String>,
    #[cfg(feature = "async-tcp")]
    pub(super) lifecycle: LifecycleHooks,
}

impl Application {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            definition: AppSchema {
                version: 1,
                app: AppDefinition { name: name.into() },
                models: BTreeMap::new(),
                model_bindings: BTreeMap::new(),
                requires: vec![],
            },
            providers: ConnectorRegistry::new(),
            deployment: None,
            catalog: None,
            selections: BTreeMap::new(),
            catalog_recovery: RecoveryPolicy::FailFast,
            selection_manifest_loaded: false,
            modules: BTreeSet::new(),
            #[cfg(feature = "async-tcp")]
            lifecycle: LifecycleHooks::default(),
        }
    }

    /// Declare an arbitrary generated Rust capability without copying its
    /// identifier or version into JSON or application logic.
    pub fn require<C: CapabilityDescriptor>(mut self, name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let capability = C::definition()?.capability;
        if let Some(previous) = self
            .definition
            .requires
            .iter()
            .find(|item| item.name == name)
        {
            if previous.capability != capability {
                return Err(SdkError::RequirementConflict(name));
            }
        } else {
            self.definition.requires.push(CapabilityRequirement {
                name,
                capability,
                depends_on: Vec::new(),
            });
        }
        Ok(self)
    }

    /// Declare startup dependencies by requirement alias. This expresses
    /// lifecycle order, not a new capability contract or provider dependency.
    pub fn depends_on<I, S>(mut self, name: &str, dependencies: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let requirement = self
            .definition
            .requires
            .iter_mut()
            .find(|requirement| requirement.name == name)
            .ok_or_else(|| SdkError::MissingCapability(name.into()))?;
        let mut declared = BTreeSet::new();
        for dependency in dependencies.into_iter().map(Into::into) {
            if requirement.depends_on.contains(&dependency) || !declared.insert(dependency.clone())
            {
                return Err(SdkError::DuplicateDependency {
                    requirement: name.into(),
                    dependency,
                });
            }
            requirement.depends_on.push(dependency);
        }
        Ok(self)
    }

    /// Declare a generated RPC capability and register its transport adapter
    /// once, even when several requirements share the same capability.
    pub fn rpc<C: RpcClientBinding + 'static>(mut self, name: impl Into<String>) -> Result<Self> {
        let capability = C::definition()?.capability;
        self = self.require::<C>(name)?;
        if self.providers.has_connector(&capability) {
            if !self.providers.connector_is::<RpcConnector<C>>(&capability) {
                return Err(SdkError::ConnectorModeConflict(capability.id));
            }
        } else {
            self.providers.register(RpcConnector::<C>::new())?;
        }
        Ok(self)
    }

    /// Register a typed async RPC requirement. Preflight and remote contract
    /// verification reuse the existing provider resolver; actual operations
    /// use nonblocking Tokio transport and the current session token.
    #[cfg(feature = "async-tcp")]
    pub fn async_rpc<C: AsyncRpcClientBinding + 'static>(
        mut self,
        name: impl Into<String>,
    ) -> Result<Self> {
        let capability = C::definition()?.capability;
        self = self.require::<C>(name)?;
        if self.providers.has_connector(&capability) {
            if !self
                .providers
                .connector_is::<AsyncRpcConnector<C>>(&capability)
            {
                return Err(SdkError::ConnectorModeConflict(capability.id));
            }
        } else {
            self.providers.register(AsyncRpcConnector::<C>::new())?;
        }
        Ok(self)
    }

    /// Register a custom capability transport adapter.
    pub fn connector(
        mut self,
        connector: impl riesel_app::CapabilityConnector + 'static,
    ) -> Result<Self> {
        self.providers.register(connector)?;
        Ok(self)
    }

    /// Register an authoritative Rust model using the conventional storage alias.
    #[cfg(feature = "records")]
    pub fn records<T: RieselModel>(self) -> Result<Self> {
        self.records_at::<T>("storage")
    }

    /// Register a Rust model and explicitly name its storage requirement.
    /// Multiple storage aliases can coexist without implicit selection.
    #[cfg(feature = "records")]
    pub fn records_at<T: RieselModel>(mut self, storage_alias: impl Into<String>) -> Result<Self> {
        let storage_alias = storage_alias.into();
        let name = T::MODEL_NAME.to_owned();
        let definition = T::model_definition();
        if let Some(previous) = self.definition.models.get(&name) {
            if previous != &definition {
                return Err(SdkError::DuplicateModel(name));
            }
        } else {
            self.definition.models.insert(name.clone(), definition);
        }

        self = self.rpc::<RecordStoreContract>(storage_alias.clone())?;
        let bindings = self.definition.model_bindings.entry(name).or_default();
        if !bindings.contains(&storage_alias) {
            bindings.push(storage_alias);
        }
        Ok(self)
    }

    /// Register a single generated local capability without handwritten
    /// component metadata or provider-factory traits.
    pub fn provide<C, F>(self, id: impl Into<String>, create: F) -> Result<Self>
    where
        C: RpcClientBinding + 'static,
        F: Fn() -> C::Service + Send + Sync + 'static,
    {
        self.provider(riesel_app::local_provider::<C, F>(id, create))
    }

    /// Register a linked declarative component with generated local and
    /// managed bindings. This does not automatically discover implementations.
    pub fn component<C: DeclarativeComponent>(self) -> Result<Self> {
        let definition =
            C::component_definition().map_err(|source| RuntimeError::ProviderFactory { source })?;
        self.provider(definition)
    }

    /// Choose a local provider without requiring deployment or a transport.
    pub fn provider(mut self, factory: impl LocalProviderFactory + 'static) -> Result<Self> {
        self.providers.register_local(factory)?;
        Ok(self)
    }

    /// Explicit opt-in to ephemeral in-memory storage for development.
    /// Production applications never fall back to MemoryStore automatically.
    #[cfg(feature = "dev-memory-storage")]
    pub fn development(self) -> Result<Self> {
        let models = self.definition.models.clone();
        self.provider(
            riesel_storage_component::memory_storage_provider_with_models(models)
                .map_err(|source| RuntimeError::ProviderFactory { source })?,
        )
    }

    /// Configure an explicit local provider or external/managed service.
    pub fn deployment(mut self, deployment: DeploymentConfig) -> Self {
        self.deployment = Some(deployment);
        self
    }

    /// Declare operator-approved remote/managed placements. Linked
    /// components remain part of the same connector registry.
    pub fn catalog(mut self, catalog: ProviderCatalog) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Select a deployment placement by requirement alias. A selection is
    /// mandatory for managed/external catalog choices; linked components
    /// retain the unambiguous local default.
    pub fn select_provider(
        mut self,
        requirement: impl Into<String>,
        choice: impl Into<String>,
    ) -> Result<Self> {
        let requirement = requirement.into();
        let choice = choice.into();
        if self.selection_manifest_loaded {
            return Err(SdkError::ProviderSelectionConflict(requirement));
        }
        if let Some(previous) = self.selections.get(&requirement)
            && previous != &choice
        {
            return Err(SdkError::ProviderSelectionConflict(requirement));
        }
        self.selections.insert(requirement, choice);
        Ok(self)
    }

    /// Configure only previously registered provider placement names from
    /// an operator-controlled selection manifest, including recovery policy.
    pub fn provider_selections(mut self, config: ProviderSelectionConfig) -> Result<Self> {
        if self.selection_manifest_loaded || !self.selections.is_empty() {
            return Err(SdkError::ProviderSelectionConflict(
                "selection configuration".into(),
            ));
        }
        self.selections = config.services;
        self.catalog_recovery = config.recovery;
        self.selection_manifest_loaded = true;
        Ok(self)
    }

    /// Inspect the exact catalog-produced plan without constructing a
    /// provider, opening a socket, or launching a managed executable.
    pub fn catalog_plan(&self) -> Result<DeploymentPlan> {
        let config = self
            .selected_deployment()?
            .ok_or(SdkError::MissingProviderCatalog)?;
        self.deployment_plan(&config)
    }

    pub(super) fn selected_deployment(&self) -> Result<Option<DeploymentConfig>> {
        if self.deployment.is_some()
            && (self.catalog.is_some()
                || !self.selections.is_empty()
                || self.selection_manifest_loaded)
        {
            return Err(SdkError::ConflictingDeploymentSource);
        }
        match (&self.deployment, &self.catalog) {
            (Some(config), None) => Ok(Some(config.clone())),
            (None, Some(catalog)) => Ok(Some(catalog.deployment_from_config(
                &self.definition,
                &self.providers,
                &ProviderSelectionConfig {
                    services: self.selections.clone(),
                    recovery: self.catalog_recovery.clone(),
                },
            )?)),
            (None, None) if self.selections.is_empty() && !self.selection_manifest_loaded => {
                Ok(None)
            }
            _ => Err(SdkError::MissingProviderCatalog),
        }
    }

    pub fn definition(&self) -> &AppSchema {
        &self.definition
    }

    /// Export the canonical application schema generated from Rust types.
    /// This is suitable for tools and visual builders; it is not a second
    /// authoritative file that a Rust developer has to edit.
    pub fn definition_json(&self) -> Result<String> {
        self.definition.validate()?;
        Ok(serde_json::to_string_pretty(&self.definition)?)
    }

    /// Inspect selected providers and dependency order without starting a
    /// service, opening a socket, or invoking application business logic.
    pub fn deployment_plan(&self, config: &DeploymentConfig) -> Result<DeploymentPlan> {
        let app = App::from_definition(self.definition.clone())?;
        Ok(app.preflight(config, &self.providers)?)
    }

    /// Async handler startup awaits native async connector inspection using
    /// the same application schema, selected providers and recovery policy.
    #[cfg(feature = "async-tcp")]
    pub(super) fn run_async_entrypoint(
        self,
        mut entrypoint: impl FnMut(
            &Context<'_>,
            &mut LifecycleHooks,
        ) -> std::result::Result<(), ConnectorError>,
    ) -> Result<()> {
        let deployment = self.selected_deployment()?;
        let mut lifecycle = self.lifecycle;
        lifecycle.order(&self.definition)?;
        let app = App::from_definition(self.definition)?;
        let invoke = |running: &RunningApp| entrypoint(&Context { running }, &mut lifecycle);
        if let Some(deployment) = deployment {
            app.run_with_async_startup(&deployment, &self.providers, invoke)?;
        } else {
            app.run_local(&self.providers, invoke)?;
        }
        Ok(())
    }

    pub fn run(
        self,
        mut entrypoint: impl FnMut(&Context<'_>) -> std::result::Result<(), ConnectorError>,
    ) -> Result<()> {
        #[cfg(feature = "async-tcp")]
        if !self.lifecycle.is_empty() {
            return Err(SdkError::AsyncLifecycleRequiresAsyncEntrypoint);
        }
        let deployment = self.selected_deployment()?;
        let app = App::from_definition(self.definition)?;
        let invoke = |running: &RunningApp| entrypoint(&Context { running });
        if let Some(deployment) = deployment {
            app.run(&deployment, &self.providers, invoke)?;
        } else {
            app.run_local(&self.providers, invoke)?;
        }
        Ok(())
    }
}
