//! Wie viele gerade spielen.
//!
//! Der Launcher schickt alle 60 Sekunden ein Lebenszeichen an den Raum-Server. Der
//! zaehlt jeden Rechner fuenf Minuten lang mit, sobald er sich einmal gemeldet hat —
//! kein Konto, die Kennung ist der Hash des Tokens, also ein Rechner. Denselben Stand
//! legen wir als Datei neben die Daten, damit der Kern ihn im Spiel anzeigen kann.

use std::time::Duration;

use swofty_core::online::multiplayer::rooms::{Api, Presence};

/// So oft der Launcher sein Lebenszeichen schickt.
pub(crate) const TAKT: Duration = Duration::from_secs(60);

/// Einmal melden. `im_spiel` sagt, ob F1 25 gerade laeuft.
pub(crate) fn melden(server: &str, token: &str, im_spiel: bool) -> Result<Presence, String> {
    Api::with_server(server, token)
        .presence(im_spiel)
        .map_err(|e| e.message)
}

/// Den Stand als Datei ablegen, die der Kern liest.
pub(crate) fn ablegen(ordner: &std::path::Path, stand: Presence) -> std::io::Result<()> {
    let text = format!(
        "{{\"launcher\":{},\"ingame\":{}}}\n",
        stand.launcher, stand.ingame
    );
    std::fs::write(ordner.join("presence.json"), text)
}

/// Die Zeile in der Oberflaeche.
pub(crate) fn text(stand: Presence) -> String {
    match (stand.launcher, stand.ingame) {
        (0, _) => "Gerade spielt niemand".to_owned(),
        (1, 1) => "Du bist gerade im Rennen".to_owned(),
        (n, 0) => format!("{n} im Launcher"),
        (n, 1) => format!("{n} im Launcher · 1 im Rennen"),
        (n, m) => format!("{n} im Launcher · {m} im Rennen"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stand(launcher: u32, ingame: u32) -> Presence {
        Presence { launcher, ingame }
    }

    #[test]
    fn die_zeile_sagt_was_los_ist() {
        assert_eq!(text(stand(0, 0)), "Gerade spielt niemand");
        assert_eq!(text(stand(1, 1)), "Du bist gerade im Rennen");
        assert_eq!(text(stand(1, 0)), "1 im Launcher");
        assert_eq!(text(stand(7, 0)), "7 im Launcher");
    }

    #[test]
    fn mehrere_rechner_und_fahrer() {
        assert_eq!(text(stand(7, 1)), "7 im Launcher · 1 im Rennen");
        assert_eq!(text(stand(12, 4)), "12 im Launcher · 4 im Rennen");
        // Ein Stand ohne Launcher kann nicht vorkommen, wird aber nicht panisch.
        assert_eq!(text(stand(0, 3)), "Gerade spielt niemand");
    }

    #[test]
    fn der_stand_wird_als_datei_abgelegt() {
        let ordner = std::env::temp_dir().join("erc-presence-probe");
        std::fs::create_dir_all(&ordner).unwrap();
        ablegen(&ordner, stand(5, 2)).unwrap();
        let text = std::fs::read_to_string(ordner.join("presence.json")).unwrap();
        assert_eq!(text, "{\"launcher\":5,\"ingame\":2}\n");
        std::fs::remove_dir_all(&ordner).ok();
    }
}

mod fenster {
    // Meldet den Rechner an den Raum-Server und zeigt den Stand auf der Startseite.

    use gpui_kit::component::h_flex;
    use gpui_kit::*;

    use crate::features::presence::{self, TAKT};
    use crate::ui::scale::px;
    use crate::shell::Shell;
    use crate::ui::theme::Palette;

    impl Shell {
        /// Alle 60 Sekunden ein Lebenszeichen. Netzverkehr im Hintergrund, die
        /// Oberflaeche wartet nicht darauf.
        pub(crate) fn watch_anwesend(window: &mut Window, cx: &mut Context<Self>) {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(5))
                    .await;
                loop {
                    let auftrag = this.update(cx, |s, _| {
                        // Auch ohne Anmeldung: der eigene Schluessel dieser Installation.
                        let token = s
                            .account
                            .session
                            .as_ref()
                            .map(|m| m.token.clone())
                            .or_else(|| {
                                swofty_core::online::multiplayer::guest::token(&s.store.root).ok()
                            })?;
                        Some((
                            swofty_core::online::multiplayer::client::room_server(),
                            token,
                            !s.game.processes.is_empty(),
                        ))
                    });
                    if let Ok(Some((server, token, im_spiel))) = auftrag {
                        let stand = cx
                            .background_executor()
                            .spawn(async move { presence::melden(&server, &token, im_spiel) })
                            .await;
                        if let Ok(stand) = stand {
                            let _ = this.update(cx, |s, cx| {
                                presence::ablegen(&s.store.root, stand).ok();
                                if s.anwesend != Some(stand) {
                                    s.anwesend = Some(stand);
                                    cx.notify();
                                }
                            });
                        }
                    }
                    cx.background_executor().timer(TAKT).await;
                }
            })
            .detach();
        }

        /// Die Zeile auf der Startseite: wer gerade da ist. Solange kein Stand
        /// vorliegt, steht hier nichts — lieber leer als geraten.
        pub(crate) fn anwesend_zeile(&self, p: Palette) -> Option<AnyElement> {
            let stand = self.anwesend?;
            Some(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .w(px(6.))
                            .h(px(6.))
                            .rounded_full()
                            .bg(if stand.ingame > 0 { p.success } else { p.text_muted }),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.text_muted)
                            .child(presence::text(stand)),
                    )
                    .into_any_element(),
            )
        }
    }
}
