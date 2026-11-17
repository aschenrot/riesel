use anyhow::Result;
use riesel::prelude::*;
pub fn main() -> Result<()> {
    let app = App::load("app.json")?;

    println!("{}", app.config().name);
    Ok(())
}
