//! Linux/Unix process-boundary regression for the still-unaccepted #248
//! attachment architecture. Separate OS PIDs, real Riesel application
//! generations and graph observer; no fake principal or public exporter.
#![cfg(unix)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;

fn request(socket: &Path, message: Value) -> Value {
    let mut stream = UnixStream::connect(socket).expect("actual independent socket attachment");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(15)))
        .unwrap();
    stream
        .write_all(serde_json::to_string(&message).unwrap().as_bytes())
        .unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).unwrap();
    assert!(reply.len() <= 4096, "bounded fixture response");
    serde_json::from_slice(&reply).expect("valid server response")
}

fn snapshot(socket: &Path, generation: &str, extra: Option<&str>) -> Value {
    let mut req = json!({
        "app": "research.riesel.gui-owner",
        "interface": "research.graph.read",
        "version": 1,
        "operation": "snapshot",
        "generation": generation
    });
    if let Some(pause) = extra {
        req["research_pause"] = json!(pause);
    }
    request(socket, req)
}

fn cursor_status(socket: &Path, generation: &str, cursor: u64) -> Value {
    request(socket, json!({
        "app": "research.riesel.gui-owner",
        "interface": "research.graph.read",
        "version": 1,
        "operation": "cursor_status",
        "generation": generation,
        "cursor": cursor
    }))
}

fn status(value: &Value) -> &str {
    value["status"].as_str().expect("status required")
}

fn owner_line(reader: &mut BufReader<impl Read>, prefix: &str) -> String {
    let mut line = String::new();
    assert!(
        reader.read_line(&mut line).unwrap() > 0,
        "owner exited early"
    );
    assert!(
        line.starts_with(prefix),
        "expected owner {prefix} but received {line:?}"
    );
    line.trim().to_owned()
}

fn control(owner: &mut Child, command: &str) {
    writeln!(owner.stdin.as_mut().unwrap(), "{command}").unwrap();
    owner.stdin.as_mut().unwrap().flush().unwrap();
}

struct Host(Child);

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A second, independently launched OS process using the same Unix login
/// identity can still read the interface, even though it was never given
/// an individual client grant. Expected negative security evidence.
#[test]
fn same_uid_attacher() {
    let Ok(path) = std::env::var("GUI_RESEARCH_SOCKET") else {
        return;
    };
    let socket = Path::new(&path);
    let descriptor = request(socket, json!({"operation": "describe"}));
    assert_eq!(status(&descriptor), "descriptor");
    let generation = descriptor["generation"].as_str().unwrap();
    let result = snapshot(socket, generation, None);
    assert_eq!(status(&result), "ok");
    assert_eq!(result["completed"], 2);
    println!("SAME_UID_CLIENT_WITHOUT_DISTINCT_GRANT_ACCEPTED");
}

#[test]
fn actual_os_client_session_fencing_and_revocation() {
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("gui.sock");
    let child = Command::new(env!("CARGO_BIN_EXE_attachment_host"))
        .arg(&socket)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut host = Host(child);
    let mut reader = BufReader::new(host.0.stdout.take().unwrap());
    let ready = owner_line(&mut reader, "READY ");
    let parts: Vec<_> = ready.split_whitespace().collect();
    assert_eq!(parts.len(), 3);
    let pid = parts[1];
    let first_generation = parts[2].to_owned();
    assert_eq!(pid.parse::<u32>().unwrap(), host.0.id());

    let announced = request(&socket, json!({"operation": "describe"}));
    assert_eq!(status(&announced), "descriptor");
    assert_eq!(announced["pid"], host.0.id());
    assert_eq!(announced["generation"], first_generation);
    let first = snapshot(&socket, &first_generation, None);
    assert_eq!(status(&first), "ok");
    assert_eq!(first["completed"], 2, "real dependent operations");
    assert_eq!(first["phase"], "Finished");
    assert_eq!(first["revision"], 1);
    assert_eq!(first["plan_id"], "research.gui.session-bound");
    assert_eq!(first["session_expired"], false);
    assert_ne!(
        first["graph_generation"], first["publication_generation"],
        "run identity is not the application publication generation"
    );
    assert!(
        first["latest_sequence"].as_u64().unwrap() > 0,
        "observer must contain real runtime transitions"
    );

    let mut wrong_version = json!({
        "app": "research.riesel.gui-owner", "interface": "research.graph.read",
        "version": 2, "operation": "snapshot",
        "generation": first_generation
    });
    assert_eq!(
        status(&request(&socket, wrong_version.clone())),
        "wrong_interface"
    );
    wrong_version["version"] = json!(1);
    wrong_version["interface"] = json!("provider.internal");
    assert_eq!(status(&request(&socket, wrong_version)), "wrong_interface");
    assert_eq!(
        status(&request(
            &socket,
            json!({
                "app": "research.riesel.gui-owner",
                "interface": "research.graph.read",
                "version": 1, "operation": "activate",
                "generation": first_generation
            })
        )),
        "not_exposed",
        "UI has no graph-control or plan-activation route"
    );
    assert_eq!(
        status(&snapshot(&socket, "forged-generation", None)),
        "stale_generation"
    );

    // Real OS client B has the same UID and no individually granted identity.
    // It can independently discover the public generation and read the view.
    let sibling = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("same_uid_attacher")
        .arg("--nocapture")
        .env("GUI_RESEARCH_SOCKET", &socket)
        .output()
        .unwrap();
    assert!(
        sibling.status.success(),
        "same-UID negative test failed: {}",
        String::from_utf8_lossy(&sibling.stderr)
    );
    assert!(
        String::from_utf8_lossy(&sibling.stdout)
            .contains("SAME_UID_CLIENT_WITHOUT_DISTINCT_GRANT_ACCEPTED")
    );

    // A request stopped before admission MUST lose the race to withdrawal.
    let paused_socket = socket.clone();
    let paused_generation = first_generation.clone();
    let before =
        thread::spawn(move || snapshot(&paused_socket, &paused_generation, Some("before")));
    owner_line(&mut reader, "WAIT_BEFORE_ADMISSION");
    control(&mut host.0, "REVOKE");
    owner_line(&mut reader, "REVOKED");
    control(&mut host.0, "RESUME");
    owner_line(&mut reader, "RESUMED");
    assert_eq!(status(&before.join().unwrap()), "withdrawn");
    assert_eq!(
        status(&snapshot(&socket, &first_generation, None)),
        "withdrawn"
    );

    // The exact OS process and socket remain the same, but the real
    // RunningApp session is shut down and replaced with a new generation.
    control(&mut host.0, "RESTART");
    let restart = owner_line(&mut reader, "RESTARTED ");
    let parts: Vec<_> = restart.split_whitespace().collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[1], pid);
    let next_generation = parts[2].to_owned();
    assert_ne!(first_generation, next_generation);
    assert_eq!(
        status(&snapshot(&socket, &first_generation, None)),
        "stale_generation"
    );
    let next = snapshot(&socket, &next_generation, None);
    assert_eq!(status(&next), "ok");
    assert_eq!(next["completed"], 2);
    assert_eq!(next["phase"], "Finished");
    assert_ne!(first["graph_generation"], next["graph_generation"]);

    // An already-admitted read can finish after withdrawal. Withdrawal
    // prevents *new admission*; it does not roll back in-flight work.
    let paused_socket = socket.clone();
    let paused_generation = next_generation.clone();
    let after = thread::spawn(move || snapshot(&paused_socket, &paused_generation, Some("after")));
    owner_line(&mut reader, "WAIT_AFTER_ADMISSION");
    control(&mut host.0, "REVOKE");
    owner_line(&mut reader, "REVOKED");
    control(&mut host.0, "RESUME");
    owner_line(&mut reader, "RESUMED");
    assert_eq!(status(&after.join().unwrap()), "ok");
    assert_eq!(
        status(&snapshot(&socket, &next_generation, None)),
        "withdrawn"
    );

    // More real graph transitions than the retained 128-event ring can store:
    // recovery must use a new authoritative snapshot, never a guessed replay.
    control(&mut host.0, "OVERFLOW");
    let overflow_line = owner_line(&mut reader, "RESTARTED ");
    let overflow_parts: Vec<_> = overflow_line.split_whitespace().collect();
    assert_eq!(overflow_parts[1], pid);
    let overflow_generation = overflow_parts[2];
    assert_ne!(overflow_generation, next_generation);
    assert_eq!(
        status(&snapshot(&socket, &next_generation, None)),
        "stale_generation"
    );
    let overflow = snapshot(&socket, overflow_generation, None);
    assert_eq!(status(&overflow), "ok");
    assert_eq!(overflow["phase"], "Finished");
    assert_eq!(overflow["completed"], 80);
    assert_eq!(overflow["operations"].as_object().unwrap().len(), 80);
    assert!(
        overflow["operations"]
            .as_object()
            .unwrap()
            .values()
            .all(|state| state == "Completed")
    );
    let lost = cursor_status(&socket, overflow_generation, 0);
    assert_eq!(status(&lost), "ok");
    assert_eq!(lost["truncated"], true, "old cursor must report real ring loss");
    assert_eq!(lost["event_payload_not_exported"], true);
    assert_eq!(lost["latest_sequence"], overflow["latest_sequence"]);
    assert_eq!(lost["graph_generation"], overflow["graph_generation"]);
    assert!(lost["retained_count"].as_u64().unwrap() <= 128);
    let tip = overflow["latest_sequence"].as_u64().unwrap();
    assert!(tip > 128, "real graph must have overflowed the transition ring");
    let after_resnapshot = cursor_status(&socket, overflow_generation, tip);
    assert_eq!(after_resnapshot["truncated"], false);
    assert_eq!(after_resnapshot["retained_count"], 0);
    assert_eq!(
        status(&cursor_status(&socket, overflow_generation, tip + 1)),
        "future_cursor",
        "future and mixed-run cursors must not synthesize history"
    );

    control(&mut host.0, "STOP");
    owner_line(&mut reader, "STOPPED");
    assert!(
        host.0.wait().unwrap().success(),
        "headless owner exited cleanly"
    );
    assert!(!socket.exists(), "test-owned socket was removed on stop");

    // Negative identity test: a different process can bind the same path
    // after owner exit and copy the old descriptor fields. Path/PID/epoch
    // are claims from an unauthenticated server, not verified app identity.
    let forged_listener = UnixListener::bind(&socket).unwrap();
    let forged_generation = overflow_generation.to_owned();
    let forged_pid = pid.parse::<u32>().unwrap();
    let responder = thread::spawn(move || {
        let (mut stream, _) = forged_listener.accept().unwrap();
        let mut input = Vec::new();
        stream.read_to_end(&mut input).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&input).unwrap()["operation"],
            "describe"
        );
        let claimed = json!({
            "status": "descriptor",
            "app": "research.riesel.gui-owner",
            "interface": "research.graph.read",
            "version": 1,
            "generation": forged_generation,
            "pid": forged_pid
        });
        stream
            .write_all(serde_json::to_string(&claimed).unwrap().as_bytes())
            .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
    });
    let fake = request(&socket, json!({"operation": "describe"}));
    assert_eq!(status(&fake), "descriptor");
    assert_eq!(fake["pid"], forged_pid);
    assert_eq!(fake["generation"], overflow_generation);
    responder.join().unwrap();
    println!("GRAPH_TRACE_LOSS_RESNAPSHOT_AND_UNAUTHENTICATED_SERVER_IMPERSONATION_VERIFIED");
}
