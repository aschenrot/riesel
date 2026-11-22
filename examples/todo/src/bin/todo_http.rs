//! Local-only Todo HTTP backend; the process outlives browser connections.
use riesel::storage::RecordQueryContract;
use riesel::{Application, AsyncTasks, ModelRead, Records};
use std::{env, error::Error, io, path::PathBuf, process::ExitCode, sync::Arc};
use todo::TaskData;

fn usage() -> &'static str {
    "Usage: todo_http --db PATH [--port PORT] [--assets DIR]\n     Binds 127.0.0.1; port 0 is OS-assigned. --assets serves trusted SvelteKit build/ files."
}

fn parse() -> Result<Option<(PathBuf, u16, Option<PathBuf>)>, io::Error> {
    let mut args = env::args_os().skip(1);
    let Some(first) = args.next() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, usage()));
    };
    if first == "--help" {
        if args.next().is_some() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, usage()));
        }
        return Ok(None);
    }
    if first != "--db" {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, usage()));
    }
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, usage()))?,
    );
    let mut port = None;
    let mut assets = None;
    while let Some(option) = args.next() {
        if option == "--port" && port.is_none() {
            port = Some(
                args.next()
                    .and_then(|x| x.into_string().ok())
                    .and_then(|x| x.parse::<u16>().ok())
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid port"))?,
            );
        } else if option == "--assets" && assets.is_none() {
            let directory = PathBuf::from(args.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "missing assets directory")
            })?);
            if directory.as_os_str().is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "empty assets directory",
                ));
            }
            assets = Some(directory);
        } else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, usage()));
        }
    }
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, usage()));
    }
    Ok(Some((path, port.unwrap_or(0), assets)))
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let Some((path, port, assets)) = parse()? else {
        println!("{}", usage());
        return Ok(());
    };
    Application::new("TodoHttp")
        .async_handler(
            move |
                records: Records<TaskData>,
                reader: ModelRead<TaskData, RecordQueryContract>,
                tasks: AsyncTasks,
            | {
                let directory = assets.clone();
                async move {
                    todo::http::serve(records, Some(Arc::new(reader)), tasks, port, directory).await
                }
            },
        )?
        .sqlite_storage(path)?
        .run()?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Todo HTTP startup failed: {error}");
            let mut source = error.source();
            while let Some(cause) = source {
                eprintln!("caused by: {cause}");
                source = cause.source();
            }
            ExitCode::from(1)
        }
    }
}
