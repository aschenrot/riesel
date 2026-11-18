use riesel::Application;
use todo::run_demo;

fn main() -> riesel::Result<()> {
    Application::new("Todo")
        .handler(run_demo)?
        .development()?
        .run_from_env()
}
