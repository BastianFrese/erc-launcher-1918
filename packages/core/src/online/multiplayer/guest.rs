//! The name a player who never signed in goes by in a room.
//!
//! Our room server takes a signed-out player under this installation's own token:
//! `guest-token` next to the rooms in the data folder, made once and then reused, so
//! the same installation keeps its name across restarts without an account.

use std::io;
use std::path::Path;

const FILE: &str = "guest-token";

/// The token for `data_dir`, made on first use. A signed-in player never needs it.
pub fn token(data_dir: &Path) -> io::Result<String> {
    let path = data_dir.join("multiplayer").join(FILE);
    if let Ok(text) = std::fs::read_to_string(&path) {
        let text = text.trim();
        if text.len() >= 16 && text.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Ok(text.to_owned());
        }
    }
    let mut raw = [0u8; 16];
    crate::online::server::sign_in::getrandom(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &token)?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_installation_keeps_its_token() {
        let dir = std::env::temp_dir().join(format!("swofty-guest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = token(&dir).unwrap();
        let again = token(&dir).unwrap();
        assert_eq!(first, again);
        assert_eq!(first.len(), 32);
        assert!(first.bytes().all(|b| b.is_ascii_alphanumeric()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrecked_file_is_replaced() {
        let dir = std::env::temp_dir().join(format!("swofty-guest-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("multiplayer")).unwrap();
        std::fs::write(dir.join("multiplayer").join(FILE), "kurz").unwrap();
        let made = token(&dir).unwrap();
        assert_eq!(made.len(), 32);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
