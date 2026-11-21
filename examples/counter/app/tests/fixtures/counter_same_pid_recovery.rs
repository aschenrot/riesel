//! Research-only same-OS-process whole-session recovery with a real Riesel-
//! resolved Counter. This test-only adapter is not a published SDK API and
//! grants no meaningful hostile-same-UID authentication.
use riesel::Application;
use riesel_counter::{Counter, CounterContract};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::error::Error;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const APP: &str = "research.counter.publisher";
const INTERFACE: &str = "counter.public";

type CounterHandle = Arc<dyn Counter + Send + Sync>;

#[derive(Default)]
struct PublishedSession {
    generation: String,
    phase: usize,
    counter: Option<CounterHandle>,
    #[cfg(feature = "research-runtime-epoch")]
    runtime_session: Option<riesel_protocol::SessionToken>,
    attachments: HashMap<String, bool>,
}

fn denied(reason: &str) -> Value {
    json!({"error": reason})
}

fn request(state: &Mutex<PublishedSession>, stop: &AtomicBool, msg: Value) -> Value {
    if msg.get("app").and_then(Value::as_str) != Some(APP) {
        return denied("wrong_app");
    }
    if msg.get("interface").and_then(Value::as_str) != Some(INTERFACE) {
        return denied("wrong_interface");
    }
    if msg.get("version").and_then(Value::as_u64) != Some(1) {
        return denied("unsupported_version");
    }
    let mut current = state.lock().expect("research state lock poisoned");
    if msg["type"] == "status" {
        return json!({"phase":current.phase,"pid":std::process::id()});
    }
    if msg["type"] == "attach" {
        if current.counter.is_none() {
            return denied("not_ready");
        }
        #[cfg(feature = "research-runtime-epoch")]
        let _admission = match current
            .runtime_session
            .as_ref()
            .and_then(|token| token.research_admit(&current.generation))
        {
            Some(permit) => permit,
            None => return denied("not_ready"),
        };
        let write = match msg["proof"].as_str() {
            Some("research-write-NOT-A-CREDENTIAL") => true,
            Some("research-read-NOT-A-CREDENTIAL") => false,
            _ => return denied("unauthorized"),
        };
        let attachment = Uuid::new_v4().to_string();
        current.attachments.insert(attachment.clone(), write);
        let mut published = json!({"generation":current.generation,"attachment":attachment,
                       "scope":if write {"write"} else {"read"}});
        #[cfg(feature = "research-runtime-epoch")]
        {
            published["research_issuer"] = json!("runtime_session_token");
        }
        return published;
    }
    if msg["type"] != "call" && msg["type"] != "stop" {
        return denied("invalid_request");
    }
    if msg["generation"].as_str() != Some(current.generation.as_str()) {
        return denied("stale_generation");
    }
    // Research-only: lock the actual runtime token's admission gate through
    // this short synchronous dispatched operation. The runtime's invalidation
    // uses the corresponding write lock, so its completion fences new calls.
    #[cfg(feature = "research-runtime-epoch")]
    let _admission = match current.runtime_session.as_ref().and_then(|token| {
        token.research_admit(msg["generation"].as_str().unwrap_or_default())
    }) {
        Some(permit) => permit,
        None => return denied("stale_generation"),
    };
    let Some(attachment) = msg["attachment"].as_str() else {
        return denied("unauthorized");
    };
    let Some(write) = current.attachments.get(attachment).copied() else {
        return denied("unauthorized");
    };
    if msg["type"] == "stop" {
        if !write {
            return denied("forbidden");
        }
        stop.store(true, Ordering::Release);
        return json!({"ok":true});
    }
    let Some(counter) = &current.counter else {
        return denied("not_ready");
    };
    match msg["operation"].as_str() {
        Some("value") => match counter.value() {
            Ok(value) => json!({"result":{"Ok":value}}),
            Err(error) => json!({"error":"backend_error","detail":error.to_string()}),
        },
        Some("add") if !write => denied("forbidden"),
        Some("add") => {
            let Some(amount) = msg["arguments"]["amount"].as_u64() else {
                return denied("invalid_arguments");
            };
            match counter.add(amount) {
                Ok(value) => json!({"result":{"Ok":value}}),
                Err(error) => json!({"error":"backend_error","detail":error.to_string()}),
            }
        }
        _ => denied("not_published"),
    }
}

fn serve_once(
    mut socket: TcpStream,
    state: &Mutex<PublishedSession>,
    stop: &AtomicBool,
) -> io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut line = String::new();
    let mut reader = BufReader::new(socket.try_clone()?);
    if reader.read_line(&mut line)? > 65536 {
        return Err(io::Error::other("frame too long"));
    }
    let parsed: Value = serde_json::from_str(&line)?;
    let result = request(state, stop, parsed);
    socket.write_all(serde_json::to_string(&result)?.as_bytes())?;
    socket.write_all(b"\n")?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let state = Arc::new(Mutex::new(PublishedSession::default()));
    let stopping = Arc::new(AtomicBool::new(false));
    let thread_state = Arc::clone(&state);
    let thread_stop = Arc::clone(&stopping);
    let serving = thread::spawn(move || {
        while !thread_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((socket, _)) => {
                    let _ = serve_once(socket, &thread_state, &thread_stop);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("research listener failed: {error}"),
            }
        }
    });
    println!("RESEARCH_SAME_PID_BOUND {address} {}", std::process::id());
    io::stdout().flush()?;

    let closure_state = Arc::clone(&state);
    let closure_stop = Arc::clone(&stopping);
    let mut invocations = 0;
    let outcome = Application::new("SamePidRecoveryResearch")
        .rpc::<CounterContract>("counter")?
        .run_from_env(move |ctx| {
            invocations += 1;
            let counter: CounterHandle = ctx.capability::<CounterContract>("counter")?.clone();
            {
                let mut current = closure_state.lock().expect("research state lock poisoned");
                current.counter = Some(counter);
                // Existing #92 baseline was app-authored UUID issuance.
                // With the gated experiment, this generation comes from
                // StartupState::new -> RunningApp::session_token instead.
                #[cfg(feature = "research-runtime-epoch")]
                {
                    let token = ctx.research_publication_session();
                    current.generation = token.research_generation();
                    current.runtime_session = Some(token);
                }
                #[cfg(not(feature = "research-runtime-epoch"))]
                {
                    current.generation = Uuid::new_v4().to_string();
                }
                current.attachments.clear();
                current.phase = invocations;
            }
            if invocations == 1 {
                if !ctx
                    .cancellation_token()
                    .wait_cancelled_timeout(Duration::from_secs(12))
                {
                    return Err(Box::new(io::Error::other("managed provider did not crash"))
                        as riesel::ConnectorError);
                }
                let mut current = closure_state.lock().expect("research state lock poisoned");
                current.counter = None;
                current.attachments.clear();
            } else if invocations == 2 {
                for _ in 0..600 {
                    if closure_stop.load(Ordering::Acquire) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                if !closure_stop.load(Ordering::Acquire) {
                    return Err(Box::new(io::Error::other("external client did not finish"))
                        as riesel::ConnectorError);
                }
            } else {
                return Err(Box::new(io::Error::other("unexpected extra session"))
                    as riesel::ConnectorError);
            }
            Ok::<(), riesel::ConnectorError>(())
        });
    stopping.store(true, Ordering::Release);
    serving.join().expect("research server thread panicked");
    outcome?;
    Ok(())
}
