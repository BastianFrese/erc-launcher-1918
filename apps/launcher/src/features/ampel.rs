//! Die Ampel: sind unsere Dienste erreichbar?
//!
//! Drei Fragen, drei Punkte auf der Startseite — Raum-Server, Relay, Download.
//! Damit ist „bei mir geht nichts" in zwei Sekunden beantwortet. Geprueft wird
//! alle zwei Minuten im Hintergrund; die Oberflaeche wartet nie darauf.

use std::time::Duration;

/// Der Zustand eines Dienstes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tri {
    #[default]
    Unbekannt,
    Gruen,
    Rot,
}

/// Wie es gerade um unsere Dienste steht.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Stand {
    pub raum: Tri,
    pub relay: Tri,
    pub download: Tri,
    /// Wie viele Lobbys der Relay gerade kennt, wenn er antwortet.
    pub lobbys: usize,
}

impl Tri {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Unbekannt => "…",
            Self::Gruen => "erreichbar",
            Self::Rot => "nicht erreichbar",
        }
    }
}

/// Klopft beim Relay an: antwortet es ueberhaupt? Die Zahl der offenen Lobbys kommt
/// spaeter aus der Lobby-Seite; hier zaehlt nur, ob der Dienst steht.
///
/// Dieselbe Anfrage wie in der 1.4.4: `ERC-LOBBYS` an den Relay aus `ERC_RELAY`
/// beziehungsweise `f1.erdi-erc.de:35501`. Die Antwort ist eine Kopfzeile und je
/// Raum eine Zeile `code|name|spieler|kategorie`.
fn relay_probe() -> Result<(Tri, usize), String> {
    use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
    let adresse = swofty_core::online::multiplayer::relay::default_relay();
    let mit_port = if adresse.contains(':') {
        adresse
    } else {
        format!("{adresse}:35501")
    };
    let ziel: SocketAddr = match mit_port.to_socket_addrs().ok().and_then(|mut a| a.next()) {
        Some(addr) => addr,
        // Kein auflösbarer Name: derselbe rote Punkt wie ein stummer Relay.
        None => return Ok((Tri::Rot, 0)),
    };
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("Kein Sende-Socket: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(4))).ok();
    s.send_to(b"ERC-LOBBYS", ziel)
        .map_err(|e| format!("Die Anfrage ging nicht raus: {e}"))?;
    let mut puffer = [0u8; 8192];
    match s.recv_from(&mut puffer) {
        Ok((n, _)) => {
            let text = String::from_utf8_lossy(&puffer[..n]);
            let lobbys = text
                .lines()
                .skip(1)
                .filter(|zeile| zeile.split('|').count() >= 3)
                .count();
            Ok((Tri::Gruen, lobbys))
        }
        Err(_) => Ok((Tri::Rot, 0)),
    }
}

const TIMEOUT: Duration = Duration::from_secs(6);

/// Eine Adresse anklopfen: 200 bis 399 gilt als erreichbar.
fn erreichbar(adresse: &str) -> Tri {
    let gesendet = ureq::get(adresse)
        .header("User-Agent", concat!("erc-launcher/", env!("CARGO_PKG_VERSION")))
        .config()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .call();
    match gesendet {
        Ok(antwort) if antwort.status().as_u16() < 400 => Tri::Gruen,
        Ok(_) => Tri::Rot,
        Err(_) => Tri::Rot,
    }
}

/// Alles drei pruefen. Blockiert — gehoert in den Hintergrund.
pub(crate) fn pruefen() -> Stand {
    let raum = erreichbar(&format!(
        "{}/v1/presence",
        swofty_core::online::multiplayer::client::room_server()
    ));
    let download = erreichbar("https://download.erdi-erc.de/updates.json");
    let (relay, lobbys) = relay_probe().unwrap_or((Tri::Rot, 0));
    Stand {
        raum,
        relay,
        download,
        lobbys,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jeder_zustand_hat_einen_text() {
        assert_eq!(Tri::Unbekannt.text(), "…");
        assert_eq!(Tri::Gruen.text(), "erreichbar");
        assert_eq!(Tri::Rot.text(), "nicht erreichbar");
    }

    #[test]
    fn der_vorgabestand_ist_unbekannt() {
        let stand = Stand::default();
        assert_eq!(stand.raum, Tri::Unbekannt);
        assert_eq!(stand.lobbys, 0);
    }
}

mod fenster {
    // Prueft die Dienste im Hintergrund und zeigt den Stand auf der Startseite.

    use gpui_kit::component::h_flex;
    use gpui_kit::prelude::FluentBuilder;
    use gpui_kit::*;

    use crate::features::ampel::{self, Stand, Tri};
    use crate::shell::Shell;
    use crate::ui::scale::px;
    use crate::ui::theme::Palette;

    /// So oft nachgesehen wird.
    const TAKT: std::time::Duration = std::time::Duration::from_secs(120);

    impl Shell {
        pub(crate) fn watch_ampel(window: &mut Window, cx: &mut Context<Self>) {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(8))
                    .await;
                loop {
                    let stand = cx.background_executor().spawn(async { ampel::pruefen() }).await;
                    let _ = this.update(cx, |s, cx| {
                        if s.ampel != stand {
                            s.ampel = stand;
                            cx.notify();
                        }
                    });
                    cx.background_executor().timer(TAKT).await;
                }
            })
            .detach();
        }

        /// Drei Punkte mit Namen, dazu die offenen Lobbys — solange nichts
        /// geprueft ist, steht hier nichts.
        pub(crate) fn ampel_zeile(&self, p: Palette) -> Option<AnyElement> {
            if self.ampel == Stand::default() {
                return None;
            }
            let punkt = |stand: Tri| {
                div().w(px(6.)).h(px(6.)).rounded_full().bg(match stand {
                    Tri::Gruen => p.success,
                    Tri::Rot => p.error,
                    Tri::Unbekannt => p.text_muted,
                })
            };
            let eintrag = |name: &'static str, stand: Tri| {
                h_flex()
                    .items_center()
                    .gap(px(4.))
                    .child(punkt(stand))
                    .child(div().text_size(px(11.)).text_color(p.text_muted).child(name))
            };
            Some(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(12.))
                    .child(eintrag("Raum", self.ampel.raum))
                    .child(eintrag("Relay", self.ampel.relay))
                    .child(eintrag("Download", self.ampel.download))
                    .when(self.ampel.lobbys > 0, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(p.text_muted)
                                .child(format!("{} Lobbys offen", self.ampel.lobbys)),
                        )
                    })
                    .into_any_element(),
            )
        }
    }
}
