//! Todo R1: explicit persistent, headless task management.
//! Keep the in-memory R0 demo and its managed-process smoke tests separate.
use riesel::{Application, Records};
use std::{env, error::Error, ffi::OsString, path::PathBuf, process::ExitCode};
use todo::{Task, TaskData, Todo, TodoError};
use uuid::Uuid;

const USAGE: &str = "Todo — persistent task manager

Usage:
  todo_cli --db PATH add TITLE
  todo_cli --db PATH list
  todo_cli --db PATH show UUID
  todo_cli --db PATH edit UUID REV TITLE
  todo_cli --db PATH complete UUID REV
  todo_cli --db PATH reopen UUID REV
  todo_cli --db PATH delete UUID REV
  todo_cli --help

REV is the revision returned by the last successful add, show, or list.
The database path is explicit and its parent directory must exist.";

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Run(#[from] riesel::SdkError),
}

#[derive(Debug)]
enum Command {
    Add(String),
    List,
    Show(Uuid),
    Edit {
        id: Uuid,
        revision: u64,
        title: String,
    },
    Complete {
        id: Uuid,
        revision: u64,
        completed: bool,
    },
    Delete {
        id: Uuid,
        revision: u64,
    },
}

fn usage(message: impl Into<String>) -> CliError {
    CliError::Usage(message.into())
}

fn string(argument: OsString) -> Result<String, CliError> {
    argument
        .into_string()
        .map_err(|_| usage("command arguments must be UTF-8"))
}

fn id(argument: OsString) -> Result<Uuid, CliError> {
    let argument = string(argument)?;
    Uuid::parse_str(&argument).map_err(|_| usage(format!("invalid task UUID: {argument}")))
}

fn revision(argument: OsString) -> Result<u64, CliError> {
    let argument = string(argument)?;
    argument
        .parse()
        .map_err(|_| usage(format!("invalid revision: {argument}")))
}

fn title(arguments: Vec<OsString>) -> Result<String, CliError> {
    if arguments.is_empty() {
        return Err(usage("missing task title"));
    }
    let title = arguments
        .into_iter()
        .map(string)
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
    if title.trim().is_empty() {
        return Err(usage("task title must not be empty"));
    }
    Ok(title)
}

fn parse(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Option<(PathBuf, Command)>, CliError> {
    let mut args = arguments.into_iter();
    let Some(first) = args.next() else {
        return Err(usage("missing --db PATH and command"));
    };
    if first == "--help" || first == "-h" {
        if args.next().is_some() {
            return Err(usage("--help takes no arguments"));
        }
        return Ok(None);
    }
    if first != "--db" {
        return Err(usage("expected --db PATH (or --help)"));
    }
    let path = PathBuf::from(args.next().ok_or_else(|| usage("missing database path"))?);
    if path.as_os_str().is_empty() {
        return Err(usage("database path must not be empty"));
    }
    let name = string(args.next().ok_or_else(|| usage("missing command"))?)?;
    let mut rest = args.collect::<Vec<_>>().into_iter();
    let command = match name.as_str() {
        "add" => Command::Add(title(rest.by_ref().collect())?),
        "list" => Command::List,
        "show" => Command::Show(id(rest.next().ok_or_else(|| usage("missing UUID"))?)?),
        "edit" => {
            let id = id(rest.next().ok_or_else(|| usage("missing UUID"))?)?;
            let revision = revision(rest.next().ok_or_else(|| usage("missing revision"))?)?;
            Command::Edit {
                id,
                revision,
                title: title(rest.by_ref().collect())?,
            }
        }
        "complete" | "reopen" | "delete" => {
            let id = id(rest.next().ok_or_else(|| usage("missing UUID"))?)?;
            let revision = revision(rest.next().ok_or_else(|| usage("missing revision"))?)?;
            match name.as_str() {
                "delete" => Command::Delete { id, revision },
                "complete" => Command::Complete {
                    id,
                    revision,
                    completed: true,
                },
                _ => Command::Complete {
                    id,
                    revision,
                    completed: false,
                },
            }
        }
        _ => return Err(usage(format!("unknown command: {name}"))),
    };
    // Commands with a title consumed every remaining token. Every other
    // command must reject extra arguments rather than silently ignore them.
    if rest.next().is_some() {
        return Err(usage("unexpected extra arguments"));
    }
    Ok(Some((path, command)))
}

fn print_task(task: &Task) {
    println!(
        "{}\t{}\t{}\t{:?}",
        task.id,
        task.revision,
        if task.value.completed { "done" } else { "open" },
        task.value.title
    );
}

fn execute(records: Records<TaskData>, command: &Command) -> Result<(), TodoError> {
    let todo = Todo::new(records);
    match command {
        Command::Add(title) => print_task(&todo.create_task(title.clone())?),
        Command::List => {
            for task in todo.list_tasks()? {
                print_task(&task);
            }
        }
        Command::Show(id) => {
            print_task(&todo.get_task(*id)?.ok_or(TodoError::NotFound)?);
        }
        Command::Edit {
            id,
            revision,
            title,
        } => {
            print_task(&todo.edit_title(*id, *revision, title.clone())?);
        }
        Command::Complete {
            id,
            revision,
            completed,
        } => {
            print_task(&todo.set_completed(*id, *revision, *completed)?);
        }
        Command::Delete { id, revision } => {
            todo.delete_task(*id, *revision)?;
            println!("deleted\t{id}");
        }
    }
    Ok(())
}

fn run() -> Result<(), CliError> {
    let Some((path, command)) = parse(env::args_os().skip(1))? else {
        println!("{USAGE}");
        return Ok(());
    };
    Application::new("Todo")
        .handler(move |records: Records<TaskData>| execute(records, &command))?
        .sqlite_storage(path)?
        .run()?;
    Ok(())
}

fn print_error_chain(error: &dyn Error) {
    eprintln!("error: {error}");
    let mut source = error.source();
    while let Some(cause) = source {
        eprintln!("caused by: {cause}");
        source = cause.source();
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(message)) => {
            eprintln!("{message}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(error) => {
            print_error_chain(&error);
            ExitCode::from(1)
        }
    }
}
