use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Context;

use crate::app::msg::{IpcCommand, Msg, StateSnapshot};

/// Per-client write timeout. A wedged TUI must not block the daemon main loop
/// — anything slower than this is treated as a dead client and disconnected.
const BROADCAST_WRITE_TIMEOUT: Duration = Duration::from_millis(200);

/// Return the path to the Unix domain socket used for IPC.
pub fn socket_path() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        std::path::PathBuf::from(dir).join("kvn-tui.sock")
    } else {
        let uid = unsafe { libc::getuid() };
        std::path::PathBuf::from("/tmp").join(format!("kvn-tui-{}.sock", uid))
    }
}

/// Remove the socket file.
pub fn cleanup_socket() {
    let _ = std::fs::remove_file(socket_path());
}

/// Check whether the daemon socket is accepting connections.
pub fn is_daemon_running() -> bool {
    UnixStream::connect(socket_path()).is_ok()
}

/// Daemon-side IPC server.
pub struct IpcServer {
    clients: Arc<Mutex<Vec<UnixStream>>>,
}

impl IpcServer {
    pub fn bind(tx: Sender<Msg>) -> anyhow::Result<Self> {
        let path = socket_path();
        if path.exists() {
            std::fs::remove_file(&path).ok();
        }
        let listener = UnixListener::bind(&path)?;
        let clients: Arc<Mutex<Vec<UnixStream>>> = Arc::new(Mutex::new(Vec::new()));
        let clients_clone = clients.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let writer = match stream.try_clone() {
                            Ok(w) => w,
                            Err(e) => {
                                tracing::warn!("Failed to clone IPC stream for broadcast: {e}");
                                continue;
                            }
                        };
                        if let Err(e) = writer.set_write_timeout(Some(BROADCAST_WRITE_TIMEOUT)) {
                            tracing::warn!("Failed to set IPC write timeout: {e}");
                        }
                        let tx = tx.clone();
                        let clients = clients_clone.clone();
                        clients.lock().unwrap().push(writer);
                        thread::spawn(move || {
                            let reader = BufReader::new(stream);
                            for line in reader.lines() {
                                match line {
                                    Ok(line) => {
                                        if let Ok(cmd) = serde_json::from_str::<IpcCommand>(&line) {
                                            let _ = tx.send(Msg::IpcCommand(cmd));
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        });
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self { clients })
    }

    /// Send a state snapshot to every connected TUI client.
    ///
    /// Writes are performed *without* holding the clients mutex: one slow or
    /// wedged TUI must not stall the daemon main loop or other clients.
    /// Per-stream `set_write_timeout` provides the upper bound on how long
    /// a single client can hold us up.
    pub fn broadcast(&self, snapshot: &StateSnapshot) {
        let json = match serde_json::to_string(snapshot) {
            Ok(s) => s + "\n",
            Err(e) => {
                tracing::warn!("Failed to serialize state snapshot: {e}");
                return;
            }
        };

        // Dup the stream fds under a short lock so writes happen without it.
        // We tag each clone with the original fd so cleanup can identify dead
        // clients even if `clients` was mutated by accept-loop concurrently.
        let writers: Vec<(RawFd, UnixStream)> = {
            let guard = self.clients.lock().unwrap();
            guard
                .iter()
                .filter_map(|s| s.try_clone().ok().map(|c| (s.as_raw_fd(), c)))
                .collect()
        };

        let mut dead_fds: HashSet<RawFd> = HashSet::new();
        for (fd, mut client) in writers {
            if let Err(e) = client.write_all(json.as_bytes()) {
                tracing::debug!("Dropping IPC client (fd={fd}): {e}");
                dead_fds.insert(fd);
            }
        }

        if !dead_fds.is_empty() {
            let mut guard = self.clients.lock().unwrap();
            guard.retain(|s| !dead_fds.contains(&s.as_raw_fd()));
        }
    }
}

/// TUI client-side IPC connection.
pub struct IpcClient {
    stream: UnixStream,
}

impl IpcClient {
    pub fn connect() -> anyhow::Result<Self> {
        let stream = UnixStream::connect(socket_path())?;
        stream.set_nonblocking(false)?;
        Ok(Self { stream })
    }

    pub fn send(&mut self, cmd: &IpcCommand) -> anyhow::Result<()> {
        let json = serde_json::to_string(cmd)? + "\n";
        self.stream.write_all(json.as_bytes())?;
        self.stream.flush()?;
        Ok(())
    }

    /// Spawn a background thread that reads state snapshots from the daemon
    /// and forwards them into the given mpsc channel.
    pub fn spawn_reader(&self, tx: Sender<Msg>) -> anyhow::Result<()> {
        let stream = self
            .stream
            .try_clone()
            .context("Failed to clone IPC socket for snapshot reader")?;
        thread::spawn(move || {
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        if let Ok(snapshot) = serde_json::from_str::<StateSnapshot>(&line) {
                            let _ = tx.send(Msg::StateUpdate(snapshot));
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(())
    }
}
