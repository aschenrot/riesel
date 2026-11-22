//! Test-only offline fixture for a real sparse native Todo Query browser run.
//! Feature gated: not a normal Todo/Manager command or a production migration.
use riesel::{Application, Records};
use std::{env, error::Error, io, path::PathBuf, process::ExitCode};
use todo::{TaskData, TodoError};

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--db")) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "expected --db PATH").into());
    }
    let db = PathBuf::from(args.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "missing fixture database path")
    })?);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--count")) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "expected --count 10000").into());
    }
    if args.next().as_deref() != Some(std::ffi::OsStr::new("10000")) || args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "this isolated browser benchmark only seeds exactly 10000 records",
        )
        .into());
    }
    Application::new("TodoSparseBrowserFixture")
        .handler(|records: Records<TaskData>| -> Result<(), TodoError> {
            if !records.list_page(None, 1)?.records.is_empty() {
                return Err(TodoError::Storage(riesel::SdkError::Store(
                    riesel::storage::StoreError::Failed("fixture database must be empty".into()),
                )));
            }
            let mut rows = Vec::with_capacity(10_000);
            for index in 0..10_000 {
                rows.push(records.create(TaskData {
                    title: format!("Sparse completed task {index:05}"),
                    completed: true,
                })?);
            }
            rows.sort_by_key(|row| row.id);
            let last = rows.last().expect("10000 records were seeded");
            let opened = records.update(
                last.id,
                last.revision,
                TaskData {
                    title: "Sparse Open at UUID end".into(),
                    completed: false,
                },
            )?;
            assert_eq!(opened.id, last.id);
            assert_eq!(opened.revision, last.revision + 1);
            println!("TODO_SPARSE_SEED_OK records=10000 last_open_id={}", opened.id);
            Ok(())
        })?
        .sqlite_storage(db)?
        .run()?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Todo sparse browser fixture failed: {error}");
            ExitCode::FAILURE
        }
    }
}
