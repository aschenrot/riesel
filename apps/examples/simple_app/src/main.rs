use riesel::{App, ConnectorRegistry, DeploymentConfig};
use std::error::Error;

pub fn main() -> Result<(), Box<dyn Error>> {
    let app = App::from_schema(r#"{"version":1,"app":{"name":"Riesel"},"models":{}}"#)?;
    let deployment = DeploymentConfig::from_json(r#"{"services":{}}"#)?;
    let connectors = ConnectorRegistry::new();

    app.run(&deployment, &connectors, |running| {
        println!("Application: {}", running.app().definition().app.name);
        Ok(())
    })?;
    Ok(())
}
