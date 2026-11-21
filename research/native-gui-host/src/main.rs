//! Research-only desktop client for the already merged SDK-resolved Counter facade.
//! Deliberately no application exporter API, Manager, or native Riesel UI framework.
use eframe::egui;
use riesel_counter::AsyncCounterContract;
use riesel_protocol::{AsyncRpcClientBinding, AsyncTcpRpcTransport};
use std::{
    env,
    error::Error,
    io::{self, Write},
    net::SocketAddr,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};

const IO_DEADLINE: Duration = Duration::from_secs(2);
const BACKGROUND_POLL: Duration = Duration::from_millis(900);

fn mark(message: impl AsRef<str>) {
    println!("{}", message.as_ref());
    let _ = io::stdout().flush();
}

#[derive(Clone, Copy, Debug)]
enum Request {
    Refresh,
    Increment,
}

#[derive(Debug)]
enum Reply {
    Pending,
    Value(u64),
    Disconnected(String),
    UnknownOutcome(String),
}

fn read_or_change(
    runtime: &tokio::runtime::Runtime,
    address: SocketAddr,
    increment: bool,
    delay: Duration,
) -> Reply {
    // Deliberately model slow I/O on the worker, never on the GUI event loop.
    if !delay.is_zero() {
        thread::sleep(delay);
    }
    // Even constructing a Tokio transport may require an active reactor.
    // Keep transport creation, verification and all RPCs in this worker's
    // Tokio context; the native main thread never enters this runtime.
    runtime.block_on(async {
        let transport = Arc::new(AsyncTcpRpcTransport::new(address));
        let definition = match AsyncCounterContract::definition() {
            Ok(definition) => definition,
            Err(error) => return Reply::Disconnected(error.to_string()),
        };
        match tokio::time::timeout(IO_DEADLINE, transport.verify(&definition)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                return Reply::Disconnected(format!("capability verification: {error}"));
            }
            Err(_) => return Reply::Disconnected("verification timed out".into()),
        }

        let client = AsyncCounterContract::bind_async(transport);
        if increment {
            match tokio::time::timeout(IO_DEADLINE, client.add(1)).await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    // No transport-level response is proof of non-execution.
                    return Reply::UnknownOutcome(format!("mutation failed: {error}"));
                }
                Err(_) => return Reply::UnknownOutcome("mutation response timed out".into()),
            }
        }
        match tokio::time::timeout(IO_DEADLINE, client.value()).await {
            Ok(Ok(value)) => Reply::Value(value),
            Ok(Err(error)) => Reply::Disconnected(format!("snapshot: {error}")),
            Err(_) => Reply::Disconnected("snapshot timed out".into()),
        }
    })
}

fn worker(
    address: SocketAddr,
    delay: Duration,
    requests: Receiver<Request>,
    replies: SyncSender<Reply>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = replies.send(Reply::Disconnected(error.to_string()));
            return;
        }
    };
    let mut first = true;
    let mut outcome_unknown = false;
    loop {
        let (request, automatic) = if first {
            first = false;
            (Request::Refresh, false)
        } else {
            match requests.recv_timeout(BACKGROUND_POLL) {
                Ok(request) => (request, false),
                Err(mpsc::RecvTimeoutError::Timeout) if !outcome_unknown => {
                    (Request::Refresh, true)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        };
        if !automatic && replies.send(Reply::Pending).is_err() {
            return;
        }
        let result = read_or_change(&runtime, address, matches!(request, Request::Increment), delay);
        if matches!(result, Reply::UnknownOutcome(_)) {
            outcome_unknown = true;
        } else if matches!(result, Reply::Value(_)) {
            outcome_unknown = false;
        }
        if replies.send(result).is_err() {
            return;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Connection {
    Connecting,
    Online,
    Disconnected,
    Unknown,
}

struct Gui {
    requests: SyncSender<Request>,
    replies: Receiver<Reply>,
    state: Connection,
    last_value: Option<u64>,
    detail: String,
    busy: bool,
    focus_logged: bool,
    last_logged_state: Option<Connection>,
}

impl Gui {
    fn new(address: SocketAddr, delay: Duration) -> Self {
        let (request_tx, request_rx) = mpsc::sync_channel(4);
        let (reply_tx, reply_rx) = mpsc::sync_channel(8);
        thread::spawn(move || worker(address, delay, request_rx, reply_tx));
        mark(format!("GUI_PID {}", std::process::id()));
        Self {
            requests: request_tx,
            replies: reply_rx,
            state: Connection::Connecting,
            last_value: None,
            detail: "Connecting to explicitly supplied research endpoint".into(),
            busy: true,
            focus_logged: false,
            last_logged_state: None,
        }
    }

    fn send(&mut self, request: Request) {
        if self.busy || (self.state == Connection::Unknown && matches!(request, Request::Increment)) {
            return;
        }
        match self.requests.try_send(request) {
            Ok(()) => self.busy = true,
            Err(TrySendError::Full(_)) => self.detail = "Client worker queue is full".into(),
            Err(TrySendError::Disconnected(_)) => {
                self.state = Connection::Disconnected;
                self.detail = "Client I/O worker exited".into();
            }
        }
    }

    fn accept(&mut self, reply: Reply) {
        match reply {
            Reply::Pending => self.busy = true,
            Reply::Value(value) => {
                self.busy = false;
                self.state = Connection::Online;
                self.detail = "Snapshot from the running Riesel application".into();
                if self.last_value != Some(value) || self.last_logged_state != Some(Connection::Online) {
                    mark(format!("GUI_VALUE {value}"));
                }
                self.last_value = Some(value);
            }
            Reply::Disconnected(error) => {
                self.busy = false;
                if self.state != Connection::Unknown {
                    self.state = Connection::Disconnected;
                }
                self.detail = error;
                if self.last_logged_state != Some(Connection::Disconnected) {
                    mark("GUI_DISCONNECTED");
                }
            }
            Reply::UnknownOutcome(error) => {
                self.busy = false;
                self.state = Connection::Unknown;
                self.detail = format!("{error}; refresh before deciding whether to retry");
                mark("GUI_UNKNOWN_MUTATION_OUTCOME");
            }
        }
        self.last_logged_state = Some(self.state);
    }
}

impl eframe::App for Gui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(reply) = self.replies.try_recv() {
            self.accept(reply);
        }

        // This keyboard event is processed on the actual native event-loop
        // thread, even while the independent worker is waiting on I/O.
        if ctx.input(|input| input.key_pressed(egui::Key::H)) {
            mark("GUI_INPUT_ALIVE");
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Riesel native host — research");
            ui.label("Independent GUI process; SDK-resolved Counter stays in the headless app.");
            ui.separator();
            match self.last_value {
                Some(value) if self.state == Connection::Online => {
                    ui.label(format!("Authoritative Counter snapshot: {value}"));
                }
                Some(value) => {
                    ui.label(format!("Last snapshot (STALE): {value}"));
                }
                None => {
                    ui.label("No authoritative snapshot yet");
                }
            }
            ui.strong(format!("Connection: {:?}", self.state));
            ui.label(&self.detail);
            ui.separator();

            let increment = ui.add_enabled(
                !self.busy && self.state == Connection::Online,
                egui::Button::new("Add 1 (Ctrl+I)"),
            );
            let refresh = ui.add_enabled(!self.busy, egui::Button::new("Refresh (Ctrl+R)"));
            if (increment.has_focus() || refresh.has_focus()) && !self.focus_logged {
                mark("GUI_KEYBOARD_FOCUS");
                self.focus_logged = true;
            }
            if increment.clicked()
                || ctx.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::I))
            {
                self.send(Request::Increment);
            }
            if refresh.clicked()
                || ctx.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::R))
            {
                self.send(Request::Refresh);
            }
            ui.separator();
            ui.label("Keyboard: Tab focuses actions; Ctrl+I increments; Ctrl+R refreshes; H checks responsiveness.");
            ui.small("PROVISIONAL / INSECURE: loopback + contract verification is not client authentication or external session fencing.");
        });
        ctx.request_repaint_after(Duration::from_millis(35));
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let address: SocketAddr = args.next().ok_or("usage: native-gui <loopback-address> [delay-ms]")?.parse()?;
    if !address.ip().is_loopback() {
        return Err("research endpoint must use loopback".into());
    }
    let delay_ms = args.next().map(|value| value.parse::<u64>()).transpose()?.unwrap_or(0);
    if delay_ms > 3000 || args.next().is_some() {
        return Err("expected optional adapter delay-ms <= 3000".into());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([520.0, 320.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Riesel native GUI research",
        options,
        Box::new(move |_cc| Ok(Box::new(Gui::new(address, Duration::from_millis(delay_ms))))),
    )?;
    mark("GUI_CLOSED_WITHOUT_BACKEND_STOP");
    Ok(())
}
