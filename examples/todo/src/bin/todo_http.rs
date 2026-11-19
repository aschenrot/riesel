//! Local Todo HTTP host. This is not an authenticated remote service.
use riesel::{Application, Records};
use std::{
    env,
    error::Error,
    ffi::OsString,
    net::Ipv4Addr,
    path::PathBuf,
    process::ExitCode,
};
use todo::TaskData;
use tokio::net::TcpListener;

const USAGE: &str = "Todo HTTP — local browser API

Usage:
  todo_http --db PATH [--port PORT]
  todo_http --help

The database path is required and must refer to a writable local file.
PORT defaults to 0 (an OS-assigned port). The server prints its actual URL.
Only 127.0.0.1 is bound; requests with an unexpected Host or write Origin
are rejected. Remote access is not supported.";

fn parse() -> Result<Option<(PathBuf, u16)>, String> {
    let args = env::args_os().skip(1).collect::<Vec<OsString>>();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        return Ok(None);
    }
    let mut path = None;
    let mut port = 0_u16;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        if argument == "--db" {
            if path.is_some() {
                return Err("--db may be supplied only once".into());
            }
            let next = args.next().ok_or("missing database path")?;
            if next.is_empty() {
                return Err("database path must not be empty".into());
            }
            path = Some(PathBuf::from(next));
        } else if argument == "--port" {
            let next = args.next().ok_or("missing port")?;
            let value = next.into_string().map_err(|_| "port must be UTF-8")?;
            port = value.parse().map_err(|_| "invalid TCP port")?;
        } else {
            return Err(format!("unknown argument: {}", argument.to_string_lossy()));
        }
    }
    Ok(Some((path.ok_or("missing --db PATH")?, port)))
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    if let Ok(mut terminate) = signal(SignalKind::terminate()) {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    } else {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let Some((path, port)) = parse()? else {
        println!("{USAGE}");
        return Ok(());
    };
    Application::new("Todo HTTP")
        .async_handler(move |records: Records<TaskData>| async move {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
            eprintln!("Todo HTTP: http://{}", listener.local_addr()?);
            todo::http::serve(listener, records, shutdown_signal()).await?;
            Ok::<(), Box<dyn Error + Send + Sync>>(())
        })?
        .sqlite_storage(path)?
        .run()?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            let mut cause = error.source();
            while let Some(source) = cause {
                eprintln!("caused by: {source}");
                cause = source.source();
            }
            if error.is::<String>() {
                eprintln!("\n{USAGE}");
                ExitCode::from(2)
            } else {
                ExitCode::from(1)
            }
        }
    }
}
