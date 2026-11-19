//! Local-only Todo HTTP backend; the process outlives browser connections.
use riesel::{Application, AsyncTasks, Records};
use std::{env, error::Error, io, path::PathBuf, process::ExitCode};
use todo::TaskData;

fn usage() -> &'static str {
    "Usage: todo_http --db PATH [--port PORT]\n     Binds numeric loopback 127.0.0.1; port 0 uses an OS-assigned port."
}

fn parse() -> Result<Option<(PathBuf, u16)>, io::Error> {
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
        args.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, usage()))?,
    );
    let mut port = 0;
    if let Some(option) = args.next() {
        if option != "--port" {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, usage()));
        }
        port = args
            .next()
            .and_then(|x| x.into_string().ok())
            .and_then(|x| x.parse::<u16>().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid port"))?;
    }
    if args.next().is_some() || path.as_os_str().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, usage()));
    }
    Ok(Some((path, port)))
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let Some((path, port)) = parse()? else {
        println!("{}", usage());
        return Ok(());
    };
    Application::new("TodoHttp")
        .async_handler(move |records: Records<TaskData>, tasks: AsyncTasks| async move {
            todo::http::serve(records, tasks, port).await
        })?
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
