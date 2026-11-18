mod command;
mod ship;
mod smoke;
mod validate;
use std::{env, process::ExitCode};
fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let result = match args.as_slice() {
        [command] if command == "validate" => validate::run(validate::FormatMode::Fix),
        [command, option] if command == "validate" && option == "--check" => {
            validate::run(validate::FormatMode::Check)
        }
        [command, message @ ..] if command == "ship" => ship::run(&message.join(" ")),
        [command] if command == "smoke" => smoke::run(),
        _ => Err(
            "Usage:\n  cargo validate\n  cargo validate --check\n  cargo smoke\n  cargo ship \"Commit message\""
                .to_owned(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}
