pub struct AppConfig {
    pub name: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            name: "Riesel".into(),
        }
    }
}
