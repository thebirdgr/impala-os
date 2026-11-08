//! Impala OS core: hardware-independent commands, state, backend trait,
//! demo backend and the player service loop. No UI, no I/O besides
//! `/proc/net/route` in [`net::probe`].

pub mod backend;
pub mod command;
pub mod input;
pub mod mock;
pub mod net;
pub mod service;
pub mod state;

pub use backend::{BackendError, MediaBackend};
pub use command::{Command, CommandSource, PlaylistSource};
pub use state::{format_time, AppState, BackendStatus, NetworkStatus, PlaybackState, PlayerStatus, Track};
