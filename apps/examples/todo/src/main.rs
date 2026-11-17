use riesel::prelude::*;
use anyhow::Result;
pub fn main() -> Result<()> {
    let app = App::load("app.json")?;

    println!("{}", app.config().name);
    Ok(())
}
