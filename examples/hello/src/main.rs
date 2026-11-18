use riesel::Application;

fn main() -> riesel::Result<()> {
    Application::new("Hello").run(|_| {
        println!("Hello from Riesel");
        Ok(())
    })
}
