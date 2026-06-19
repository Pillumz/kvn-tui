use crate::app::model::{ConnectionState, Overlay, TrafficStats};
use crate::config::profile::{DnsStrategy, Profile, Settings, Subscription};
use crossterm::event::KeyEvent;
use uuid::Uuid;

/// Structured error carried inside `Msg` variants.
///
/// `chain` is ordered outermost first, matching `anyhow::Error::chain()` —
/// element 0 is the high-level summary (suitable for the status bar), and
/// later elements are the root causes added through `.context(...)`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IpcError {
    pub chain: Vec<String>,
}

impl IpcError {
    #[cfg(test)]
    pub fn new<S: Into<String>>(msg: S) -> Self {
        Self {
            chain: vec![msg.into()],
        }
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, msg) in self.chain.iter().enumerate() {
            if i > 0 {
                write!(f, ": ")?;
            }
            write!(f, "{msg}")?;
        }
        Ok(())
    }
}

impl From<anyhow::Error> for IpcError {
    fn from(err: anyhow::Error) -> Self {
        Self {
            chain: err.chain().map(|c| c.to_string()).collect(),
        }
    }
}

pub enum Msg {
    Key(KeyEvent),
    Tick,
    Resize,
    GeoUpdated(GeoResult),
    GeoLastUpdated(Option<String>),
    SystemResumed,
    Connected {
        pid: u32,
    },
    ConnectFailed(IpcError),
    SubscriptionFetched {
        id: Uuid,
        result: Result<Vec<crate::config::profile::Profile>, IpcError>,
    },

    IpcCommand(IpcCommand),
    StateUpdate(StateSnapshot),
    ConfigReloaded(Result<crate::config::profile::Config, IpcError>),
    KillSwitchApplied {
        enabled: bool,
        error: Option<IpcError>,
    },
    /// Raw sample of cumulative byte counters from sing-box's Clash API,
    /// timestamped so the pure-layer can compute a per-second rate against
    /// the previous sample stored in `Model::traffic`.
    TrafficStatsUpdated {
        up_total: u64,
        down_total: u64,
        conn_count: usize,
        sampled_at_ms: u64,
    },
}

#[derive(Debug)]
pub enum GeoResult {
    Updated {
        parts: Vec<String>,
        last_updated: Option<String>,
    },
    UpToDate,
    Error(String),
}

/// Commands sent from the TUI client to the daemon over the Unix socket.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "cmd")]
pub enum IpcCommand {
    Attach,
    Detach,
    Key {
        code: String,
        char: Option<char>,
        ctrl: bool,
    },
    Paste {
        text: String,
    },
    Copied {
        name: String,
        count: usize,
    },
    ReloadConfig,
    Quit,
}

/// State snapshot pushed from the daemon to TUI clients.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StateSnapshot {
    pub connection: ConnectionState,
    pub status: String,
    pub status_is_error: bool,
    pub singbox_pid: Option<u32>,
    pub active_profile_id: Option<String>,
    pub selected: usize,
    pub routing_selected: usize,
    pub geo_region_selected: usize,
    pub dns_selected: usize,
    #[serde(default)]
    pub dns_strategy_draft: Option<DnsStrategy>,
    pub geo_updating: bool,
    pub geo_last_updated: Option<String>,
    pub overlay: Overlay,
    pub profiles: Vec<Profile>,
    pub subscriptions: Vec<Subscription>,
    pub settings: Settings,
    #[serde(default)]
    pub traffic: TrafficStats,
}
