//! Online sessions between launchers. The session logic is a server module the launcher loads
//! ([`client`]); what stays here is the data the pages render ([`types`]) and what this player's profile
//! plays ([`fingerprint`]).
//!
//! Unser eigener Mehrspieler-Weg liegt daneben: die Bruecke ueber Dateien ([`bridge`]), der Raum auf
//! unserem Server ([`rooms`], [`session`]) und der Transport ([`relay`], [`transport`], [`turn`],
//! [`stun`], [`ws`]). Er bleibt in diesem Ordner, damit ein fremder Umbau ihn nicht mitreisst.

pub mod bridge;
pub mod client;
pub mod diag;
pub mod direct;
pub mod fingerprint;
pub mod game;
pub mod guest;
pub mod matching;
pub mod progress;
pub mod relay;
pub mod rooms;
pub mod seal;
pub mod session;
pub mod stun;
#[cfg(test)]
mod testserver;
pub mod transport;
pub mod turn;
pub mod types;
pub mod ws;
