use super::*;
use std::ffi::OsString;

impl Application {
    /// Strict infrastructure CLI for applications that accept no custom args.
    /// Unknown arguments are rejected before any component is started.
    pub fn run_from_env(
        self,
        entrypoint: impl FnMut(&Context<'_>) -> std::result::Result<(), ConnectorError>,
    ) -> Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if let Some((application, _)) = self.configure_args(&args, false)? {
            application.run(entrypoint)?;
        }
        Ok(())
    }

    /// Parse Riesel infrastructure flags from the process command line while
    /// forwarding application-specific arguments to user code unchanged.
    pub fn run_from_env_with_args(
        self,
        entrypoint: impl FnMut(&Context<'_>, &[OsString]) -> std::result::Result<(), ConnectorError>,
    ) -> Result<()> {
        self.run_with_args(std::env::args_os().skip(1), entrypoint)
    }

    /// Testable CLI entrypoint: Riesel owns --deployment, --print-definition,
    /// --inspect-deployment and --help. Other tokens are application-owned.
    /// A bare -- passes through the entire remainder without flag parsing.
    pub fn run_with_args<I, A>(
        self,
        args: I,
        mut entrypoint: impl FnMut(&Context<'_>, &[OsString]) -> std::result::Result<(), ConnectorError>,
    ) -> Result<()>
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        if let Some((application, custom)) = self.configure_args(&args, true)? {
            application.run(move |ctx| entrypoint(ctx, &custom))?;
        }
        Ok(())
    }

    /// Return None for inspection/help commands that intentionally do not
    /// start an application. Parse and validate every infrastructure flag
    /// before runtime startup to avoid launching on invalid arguments.
    pub(super) fn configure_args(
        mut self,
        args: &[OsString],
        allow_custom: bool,
    ) -> Result<Option<(Self, Vec<OsString>)>> {
        let mut custom = Vec::new();
        let mut inspect_catalog = false;
        let mut inspect_deployment = None;
        let mut index = 0;
        while index < args.len() {
            let option = &args[index];
            if option == "--help" || option == "-h" {
                println!(
                    "Usage: <application> [--deployment FILE | --inspect-deployment FILE | --select-providers FILE | --inspect-catalog | --print-definition] [-- APP_ARGS...]"
                );
                return Ok(None);
            }
            if option == "--print-definition" {
                println!("{}", self.definition_json()?);
                return Ok(None);
            }
            if option == "--inspect-catalog" {
                inspect_catalog = true;
                index += 1;
                continue;
            }
            if option == "--select-providers" {
                let path = args.get(index + 1).ok_or_else(|| {
                    SdkError::Arguments("--select-providers requires a file".into())
                })?;
                let config = ProviderSelectionConfig::load(PathBuf::from(path))?;
                self = self.provider_selections(config)?;
                index += 2;
                continue;
            }
            if option == "--" {
                if !allow_custom {
                    return Err(SdkError::Arguments(
                        "application arguments are not enabled".into(),
                    ));
                }
                custom.extend_from_slice(&args[index + 1..]);
                break;
            }
            if option == "--deployment" || option == "--inspect-deployment" {
                let path = args.get(index + 1).ok_or_else(|| {
                    SdkError::Arguments(format!("{} requires a file", option.to_string_lossy()))
                })?;
                let config = DeploymentConfig::load(PathBuf::from(path))?;
                if option == "--inspect-deployment" {
                    if inspect_deployment.replace(config).is_some() {
                        return Err(SdkError::Arguments(
                            "deployment inspection was configured more than once".into(),
                        ));
                    }
                } else {
                    if self.deployment.is_some() {
                        return Err(SdkError::Arguments(
                            "deployment was configured more than once".into(),
                        ));
                    }
                    self.deployment = Some(config);
                }
                index += 2;
                continue;
            }
            if !allow_custom {
                return Err(SdkError::Arguments(format!(
                    "unknown application argument: {}",
                    option.to_string_lossy()
                )));
            }
            custom.push(option.clone());
            index += 1;
        }
        if let Some(config) = inspect_deployment {
            if inspect_catalog || self.deployment.is_some() || self.selection_manifest_loaded {
                return Err(SdkError::ConflictingDeploymentSource);
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&self.deployment_plan(&config)?)?
            );
            return Ok(None);
        }
        if inspect_catalog {
            if self.deployment.is_some() {
                return Err(SdkError::ConflictingDeploymentSource);
            }
            println!("{}", serde_json::to_string_pretty(&self.catalog_plan()?)?);
            return Ok(None);
        }
        // Validate all selected sources before starting user code or providers.
        self.selected_deployment()?;
        Ok(Some((self, custom)))
    }
}
