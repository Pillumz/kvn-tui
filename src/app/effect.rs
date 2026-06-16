use crate::config::profile::{Profile, Settings};
use uuid::Uuid;

#[derive(Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Effect {
    Connect {
        profile: Profile,
        settings: Settings,
    },
    Disconnect,
    DownloadGeo,
    DownloadGeoIfMissing,
    RefreshGeoLastUpdated,
    WriteState,
    SaveConfig,
    OpenEditor(usize),
    PasteClipboard,
    UpdateSubscription {
        id: Uuid,
    },
    BroadcastState,
    Quit,
    AppendAppLog {
        level: String,
        message: String,
    },
    ReloadConfig,
}
