//! Actual Linux/Unix-process observation boundary experiment for #248.
//! This is neither a Riesel public exporter nor an authenticated local GUI.
//! Two unrelated same-UID clients can deliberately read the same interface.
#![cfg(unix)]

use riesel_app::{
    App, AsyncOperation, ConnectorRegistry, ExecutionPlanDraft, GraphRunObserver, OperationKey,
    OperationNode, OperationOutcome, OperationRegistry, OperationState, RunningApp,
    async_operation,
};
use riesel_protocol::session::InternalPublicationSession;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::{self, Permissions};
use std::io::{self, BufRead, Read, Write};
use std::net::Shutdown;
use std::num::NonZeroUsize;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{
    Arc, Condvar, Mutex, RwLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread;
use std::time::Duration;

const MAX_REQUEST_BYTES: u64 = 4096;
const MAX_CLIENTS: usize = 16;
const APP: &str = "research.riesel.gui-owner";
const INTERFACE: &str = "research.graph.read";

struct SessionView {
    publication: InternalPublicationSession,
    observer: GraphRunObserver,
    withdrawn: Mutex<bool>,
}

struct PauseGate {
    generation: Mutex<u64>,
    wake: Condvar,
}

impl PauseGate {
    fn wait(&self, name: &str) -> bool {
        let mut guard = self.generation.lock().unwrap();
        let before = *guard;
        println!("{name}");
        io::stdout().flush().unwrap();
        while *guard == before {
            let (next, deadline) = self
                .wake
                .wait_timeout(guard, Duration::from_secs(10))
                .unwrap();
            guard = next;
            if deadline.timed_out() {
                // A missing test-harness RESUME must not admit or complete work.
                return false;
            }
        }
        true
    }

    fn resume(&self) {
        *self.generation.lock().unwrap() += 1;
        self.wake.notify_all();
    }
}

fn build_session(
    runtime: &tokio::runtime::Runtime,
    wide: bool,
) -> Result<(RunningApp, Arc<SessionView>), Box<dyn Error>> {
    let app = App::from_schema(
        r#"{"version":1,"app":{"name":"ResearchExternalGui"},"models":{},"requires":[]}"#,
    )?;
    let running = app.start_local(&ConnectorRegistry::new())?;

    let mut operations = OperationRegistry::<AsyncOperation>::new();
    let mut draft = ExecutionPlanDraft::new("research.gui.session-bound", 1);
    let operation_count = if wide { 80 } else { 2 };
    let mut predecessor: Option<String> = None;
    for index in 0..operation_count {
        let id = format!("step-{index:03}");
        let key = OperationKey::new(&id, 1)?;
        operations.register(
            key.clone(),
            async_operation(|| async { OperationOutcome::Completed }),
        )?;
        let mut node = OperationNode::new(&id, key);
        if let Some(previous) = &predecessor {
            node = node.after(previous);
        }
        draft = draft.operation(node);
        predecessor = Some(id);
    }
    let plan = draft.validate()?.bind(&operations)?;
    let runner = running.execution_graph(plan, NonZeroUsize::new(1).unwrap())?;
    let observer = runner.observer();
    let report = runtime.block_on(runner.execute());
    if !report.succeeded() {
        return Err("real SDK execution graph failed".into());
    }
    let publication = running.session_token().internal_publication_session();
    Ok((
        running,
        Arc::new(SessionView {
            publication,
            observer,
            withdrawn: Mutex::new(false),
        }),
    ))
}

fn reject(kind: &str) -> Value {
    json!({"status": kind})
}

fn respond(request: &Value, current: &RwLock<Arc<SessionView>>, paused: &PauseGate) -> Value {
    let view = current.read().unwrap().clone();
    if request.get("operation").and_then(Value::as_str) == Some("describe") {
        // This is only an untrusted endpoint announcement, not proof that
        // the server is the intended application or that the client is trusted.
        return json!({
            "status": "descriptor",
            "app": APP, "interface": INTERFACE, "version": 1,
            "generation": view.publication.generation(),
            "pid": std::process::id()
        });
    }
    if request.get("app").and_then(Value::as_str) != Some(APP)
        || request.get("interface").and_then(Value::as_str) != Some(INTERFACE)
        || request.get("version").and_then(Value::as_u64) != Some(1)
    {
        return reject("wrong_interface");
    }
    let operation = request.get("operation").and_then(Value::as_str);
    if operation != Some("snapshot") && operation != Some("cursor_status") {
        return reject("not_exposed");
    }
    let Some(generation) = request.get("generation").and_then(Value::as_str) else {
        return reject("invalid_generation");
    };
    let pause = request.get("research_pause").and_then(Value::as_str);
    if pause == Some("before") {
        if !paused.wait("WAIT_BEFORE_ADMISSION") {
            return reject("pause_timeout");
        }
    }
    // A small, short-held application policy lock makes withdrawal and
    // ingress admission one ordered decision. It is NOT held over user work.
    let gate = view.withdrawn.lock().unwrap();
    if *gate {
        return reject("withdrawn");
    }
    let permit = match view.publication.try_admit(generation) {
        Ok(permit) => permit,
        Err(e) => {
            return reject(match e {
                riesel_protocol::session::InternalAdmissionError::WrongGeneration => {
                    "stale_generation"
                }
                riesel_protocol::session::InternalAdmissionError::Expired => "expired",
                riesel_protocol::session::InternalAdmissionError::AtCapacity => "at_capacity",
            });
        }
    };
    drop(gate);
    if pause == Some("after") {
        if !paused.wait("WAIT_AFTER_ADMISSION") {
            return reject("pause_timeout");
        }
    }
    if operation == Some("cursor_status") {
        let Some(cursor) = request.get("cursor").and_then(Value::as_u64) else {
            return reject("invalid_cursor");
        };
        return match view.observer.changes_since(cursor) {
            Ok(changes) => json!({
                "status": "ok",
                "truncated": changes.truncated,
                "latest_sequence": changes.latest_sequence,
                "retained_count": changes.events.len(),
                "graph_generation": changes.identity.session_generation().to_string(),
                "run_sequence": changes.identity.run_sequence(),
                "event_payload_not_exported": true
            }),
            Err(_) => reject("future_cursor"),
        };
    }
    let snapshot = view.observer.snapshot();
    let operations: BTreeMap<String, String> = snapshot
        .operations
        .iter()
        .map(|(id, state)| (id.clone(), format!("{:?}", state.state)))
        .collect();
    let completed = snapshot
        .operations
        .values()
        .filter(|op| matches!(op.state, OperationState::Completed))
        .count();
    let result = json!({
        "status": "ok",
        "app": APP,
        "interface": INTERFACE,
        "publication_generation": view.publication.generation(),
        "graph_generation": snapshot.identity.session_generation().to_string(),
        "plan_id": snapshot.identity.plan_id(),
        "revision": snapshot.identity.revision(),
        "phase": format!("{:?}", snapshot.phase),
        "latest_sequence": snapshot.latest_sequence,
        "completed": completed,
        "operations": operations,
        "run_sequence": snapshot.identity.run_sequence(),
        "session_expired": snapshot.session_expired,
    });
    drop(permit);
    result
}

fn handle(mut stream: UnixStream, current: Arc<RwLock<Arc<SessionView>>>, paused: Arc<PauseGate>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let mut buffer = Vec::new();
    let response = match (&mut stream)
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut buffer)
    {
        Ok(_) if buffer.len() as u64 <= MAX_REQUEST_BYTES => {
            match serde_json::from_slice::<Value>(&buffer) {
                Ok(request) if request.is_object() => respond(&request, &current, &paused),
                _ => reject("invalid_request"),
            }
        }
        _ => reject("over_budget_or_unavailable"),
    };
    let _ = stream.write_all(serde_json::to_string(&response).unwrap().as_bytes());
    let _ = stream.write_all(b"\n");
    let _ = stream.shutdown(Shutdown::Write);
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os().nth(1).ok_or("expected socket path")?;
    let path = Path::new(&path);
    if path.exists() {
        return Err("refuse to unlink an existing socket path".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (mut running, first) = build_session(&runtime, false)?;
    let current = Arc::new(RwLock::new(first));
    let paused = Arc::new(PauseGate {
        generation: Mutex::new(0),
        wake: Condvar::new(),
    });
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let stopped = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let background = {
        let stopped = Arc::clone(&stopped);
        let current = Arc::clone(&current);
        let paused = Arc::clone(&paused);
        let active = Arc::clone(&active);
        thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if active.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
                            active.fetch_sub(1, Ordering::AcqRel);
                            let _ = stream.write_all(b"{\"status\":\"at_capacity\"}\n");
                            continue;
                        }
                        let current = Arc::clone(&current);
                        let paused = Arc::clone(&paused);
                        let active = Arc::clone(&active);
                        thread::spawn(move || {
                            handle(stream, current, paused);
                            active.fetch_sub(1, Ordering::AcqRel);
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        })
    };
    println!(
        "READY {} {}",
        std::process::id(),
        current.read().unwrap().publication.generation()
    );
    io::stdout().flush()?;

    // Research-owner-only stdin control. No socket client can restart or
    // revoke the Riesel application. This is NOT a production control API.
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        match line.trim() {
            "REVOKE" => {
                let view = current.read().unwrap();
                *view.withdrawn.lock().unwrap() = true;
                println!("REVOKED");
            }
            "RESUME" => {
                paused.resume();
                println!("RESUMED");
            }
            "RESTART" | "OVERFLOW" => {
                let wide = line.trim() == "OVERFLOW";
                running.shutdown()?;
                let (next, view) = build_session(&runtime, wide)?;
                running = next;
                let next_generation = view.publication.generation();
                *current.write().unwrap() = view;
                println!("RESTARTED {} {}", std::process::id(), next_generation);
            }
            "STOP" => break,
            _ => println!("UNKNOWN_CONTROL"),
        }
        io::stdout().flush()?;
    }
    stopped.store(true, Ordering::Release);
    paused.resume();
    background.join().map_err(|_| "listener thread panicked")?;
    // Test asserts there are no retained in-flight requests at teardown.
    for _ in 0..100 {
        if active.load(Ordering::Acquire) == 0 {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if active.load(Ordering::Acquire) != 0 {
        return Err("test exited with in-flight clients".into());
    }
    running.shutdown()?;
    fs::remove_file(path)?;
    println!("STOPPED");
    io::stdout().flush()?;
    Ok(())
}
