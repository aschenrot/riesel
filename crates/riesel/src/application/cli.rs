use super::*;

impl Application {
    /// Parse infrastructure flags, keeping application-specific commands separate.
    /// Supported: --deployment FILE and --help.
    pub fn run_from_env(
        mut self,
        entrypoint: impl FnMut(&Context<'_>) -> std::result::Result<(), ConnectorError>,
    ) -> Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        match args.as_slice() {
            [] => {}
            [option] if option == "--help" || option == "-h" => {
                println!(
                    "Usage: <application> [--deployment FILE | --inspect-deployment FILE | --print-definition]"
                );
                return Ok(());
            }
            [option] if option == "--print-definition" => {
                println!("{}", self.definition_json()?);
                return Ok(());
            }
            [option, path] if option == "--deployment" => {
                self.deployment = Some(DeploymentConfig::load(PathBuf::from(path))?);
            }
            [option, path] if option == "--inspect-deployment" => {
                let config = DeploymentConfig::load(PathBuf::from(path))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&self.deployment_plan(&config)?)?
                );
                return Ok(());
            }
            _ => {
                return Err(SdkError::Arguments(
                    "expected --deployment FILE, --inspect-deployment FILE, --print-definition or --help".into(),
                ));
            }
        }
        self.run(entrypoint)
    }
}
