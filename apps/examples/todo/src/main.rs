use riesel::Application;
use todo::{TaskData, run_demo};

fn main() -> riesel::Result<()> {
    Application::new("Todo")
        .records::<TaskData>()?
        .development()?
        .run(run_demo)
}
