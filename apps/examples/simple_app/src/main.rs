use riesel::prelude::*;
pub fn main() -> Result<(), AppError> {
    // Setup
    let config = AppConfig {
        name: "Riesel".to_string(),
    };

    let app = App::with_config(config);

    // Launch
    app.run()
}
