use crate::AppConfig;

pub struct App {
    config: AppConfig,
}

#[derive(Debug)]
pub enum AppError {
    Unknown,
}

impl App {
    pub fn new() -> App {
        Self::with_config(AppConfig::default())
    }

    pub fn with_config(config: AppConfig) -> App {
        App { config }
    }

    pub fn from_schema() -> App {
        App::default();
        todo!()
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn run(self) -> Result<(), AppError> {
        Ok(())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
