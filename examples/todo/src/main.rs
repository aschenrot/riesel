use riesel::Application;
use todo::{TaskData, run_demo};

fn main() -> riesel::Result<()> {
    Application::new("Todo")
        .records::<TaskData>()?
        .development()?
        .run_from_env(run_demo)
}
