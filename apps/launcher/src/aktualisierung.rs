//! Holt neue Fassungen des Launchers von unserem eigenen Server.
//!
//! Der Launcher fragt beim Start `updates.json` ab. Steht dort eine hoehere
//! Fassung, zeigt er es an; auf Knopfdruck laedt er das Setup, prueft die
//! SHA-256 und startet es. Danach beendet er sich, damit der Installer die
//! laufende .exe ersetzen kann.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Wo die Fassungsliste liegt. Ueberschreibbar, damit man es testen kann.
const URL: &str = "https://download.erdi-erc.de/updates.json";
/// Dieselbe Liste fuer Vorabfassungen. Eine Beta zieht nur Betas nach — sie soll
/// nicht heimlich zur festen Fassung werden, und die feste nie eine Beta bekommen.
const URL_BETA: &str = "https://download.erdi-erc.de/updates-beta.json";
const LISTE_ENV: &str = "ERC_UPDATES_URL";
const TIMEOUT: Duration = Duration::from_secs(15);
const SETUP_NAME: &str = "erc-launcher-setup.exe";
/// Wie oft im Betrieb nachgesehen wird, damit eine neue Fassung ohne Neustart
/// auftaucht. Die Liste ist ein paar hundert Bytes.
const NACHSEHEN: Duration = Duration::from_secs(600);

/// Traegt die laufende Fassung eine Vorabendung (`1.3.0-beta.1`)?
fn ist_vorab() -> bool {
    env!("CARGO_PKG_VERSION").contains('-')
}

fn url() -> String {
    std::env::var(LISTE_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| {
            if ist_vorab() {
                URL_BETA.to_owned()
            } else {
                URL.to_owned()
            }
        })
}

#[derive(Debug, Clone, Deserialize)]
pub struct Angebot {
    pub version: String,
    pub setup: String,
    pub sha256: String,
    #[serde(default)]
    pub notiz: String,
}

#[derive(Debug, Clone)]
pub enum Stand {
    Ruhe,
    Aktuell,
    Neu(Angebot),
    Fehler(String),
}

impl Stand {
    pub fn angebot(&self) -> Option<&Angebot> {
        match self {
            Self::Neu(a) => Some(a),
            _ => None,
        }
    }
}

/// Fragt beim Server nach und vergleicht mit der laufenden Fassung.
pub fn pruefen() -> Stand {
    let roh = match hole(&url()) {
        Ok(t) => t,
        Err(e) => return Stand::Fehler(e),
    };
    let angebot: Angebot = match serde_json::from_str(&roh) {
        Ok(a) => a,
        Err(e) => return Stand::Fehler(format!("updates.json ist unlesbar: {e}")),
    };
    let Ok(ihre) = semver::Version::parse(angebot.version.trim_start_matches('v')) else {
        return Stand::Fehler(format!("Fassung '{}' ist keine Nummer", angebot.version));
    };
    let unsere = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("die eigene Fassung ist eine Nummer");
    if ihre > unsere {
        Stand::Neu(angebot)
    } else {
        Stand::Aktuell
    }
}

fn hole(adresse: &str) -> Result<String, String> {
    let mut antwort = ureq::get(adresse)
        .header(
            "User-Agent",
            concat!("erc-launcher/", env!("CARGO_PKG_VERSION")),
        )
        .config()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| format!("{adresse}: {e}"))?;
    let status = antwort.status().as_u16();
    if status >= 400 {
        return Err(format!("{adresse}: HTTP {status}"));
    }
    antwort
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .map_err(|e| format!("{adresse}: {e}"))
}

/// Laedt das Setup, prueft die Pruefsumme und gibt den Pfad zurueck.
pub fn laden(angebot: &Angebot) -> Result<PathBuf, String> {
    let ziel = std::env::temp_dir().join(SETUP_NAME);
    let antwort = ureq::get(&angebot.setup)
        .header(
            "User-Agent",
            concat!("erc-launcher/", env!("CARGO_PKG_VERSION")),
        )
        .config()
        .timeout_global(Some(Duration::from_secs(600)))
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| format!("{}: {e}", angebot.setup))?;
    let status = antwort.status().as_u16();
    if status >= 400 {
        return Err(format!("{}: HTTP {status}", angebot.setup));
    }
    let mut datei =
        std::fs::File::create(&ziel).map_err(|e| format!("{}: {e}", ziel.display()))?;
    let mut leser = antwort.into_body().into_reader();
    std::io::copy(&mut leser, &mut datei).map_err(|e| format!("Download: {e}"))?;
    drop(datei);

    let mut pruefer = Sha256::new();
    let mut datei = std::fs::File::open(&ziel).map_err(|e| format!("{e}"))?;
    let mut puffer = [0u8; 64 * 1024];
    loop {
        let n = datei.read(&mut puffer).map_err(|e| format!("{e}"))?;
        if n == 0 {
            break;
        }
        pruefer.update(&puffer[..n]);
    }
    let gerechnet: String = pruefer
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !angebot.sha256.eq_ignore_ascii_case(&gerechnet) {
        let _ = std::fs::remove_file(&ziel);
        return Err(format!(
            "Die Pruefsumme stimmt nicht. Erwartet {}, bekommen {gerechnet}.",
            angebot.sha256
        ));
    }
    Ok(ziel)
}

/// Startet das geladene Setup. Danach muss der Launcher sich beenden.
/// Spielt das geladene Setup still ein und startet den Launcher danach neu.
///
/// Der Installer kann die laufende `.exe` nicht ersetzen, also endet dieser
/// Prozess gleich danach. Das Setup wartet, bis die Datei frei ist, kopiert und
/// startet den Launcher am Ende selbst wieder (`.onInstSuccess` im Skript).
pub fn einspielen(setup: &PathBuf) -> Result<(), String> {
    std::process::Command::new(setup)
        .arg("/S")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Das Setup liess sich nicht starten: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eine_vorabfassung_zieht_ihre_eigene_liste() {
        // Die feste Fassung liest updates.json, die Beta updates-beta.json.
        assert!(url().ends_with(".json"));
        if ist_vorab() {
            assert!(url().ends_with("updates-beta.json"), "{}", url());
        } else {
            assert!(url().ends_with("/updates.json"), "{}", url());
        }
    }

    #[test]
    fn eine_neuere_fassung_ist_ein_angebot_und_eine_gleiche_nicht() {
        let gleich = Angebot {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            setup: "https://example/x.exe".into(),
            sha256: "00".into(),
            notiz: String::new(),
        };
        let ihre = semver::Version::parse(&gleich.version).unwrap();
        assert!(!(ihre > ihre), "die eigene Fassung ist kein Angebot");

        let hoeher = semver::Version::parse("99.0.0").unwrap();
        assert!(hoeher > ihre);
    }

    #[test]
    fn das_angebot_wird_gelesen() {
        let roh = r#"{"version":"9.9.9","setup":"https://x/y.exe","sha256":"ab","notiz":"test"}"#;
        let a: Angebot = serde_json::from_str(roh).unwrap();
        assert_eq!(a.version, "9.9.9");
        assert_eq!(a.notiz, "test");
    }
}

mod fenster {
    // Prueft beim Start, ob es eine neue Fassung gibt, und holt sie auf Knopfdruck
    // von unserem Server.

    use gpui_kit::component::button::{Button, ButtonVariants};
    use gpui_kit::assets::IconName;
    use gpui_kit::component::{Disableable, Sizable, v_flex};
    use gpui_kit::prelude::FluentBuilder;
    use gpui_kit::*;

    use crate::aktualisierung::{self, Stand};
    use crate::shell::Shell;
    use crate::theme::Palette;

    impl Shell {
        /// Beim Start und danach alle zehn Minuten nachsehen: eine Fassung, die
        /// waehrend des Betriebs erscheint, bekommt ihren Knopf ohne Neustart.
        /// Netzverkehr im Hintergrund, die Oberflaeche wartet nicht darauf.
        pub(crate) fn watch_aktualisierung(window: &mut Window, cx: &mut Context<Self>) {
            cx.spawn_in(window, async move |this, cx| {
                // Dem Fenster Zeit geben, sich zu zeigen.
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
                loop {
                    let stand = cx
                        .background_executor()
                        .spawn(async { aktualisierung::pruefen() })
                        .await;
                    // Ein Netzfehler darf ein Angebot, das schon dasteht, nicht wegnehmen.
                    let behalten = matches!(&stand, Stand::Neu(_));
                    let offen = this.update_in(cx, |s, _, cx| {
                        if behalten || !matches!(s.aktualisierung, Stand::Neu(_)) {
                            s.aktualisierung = stand;
                            cx.notify();
                        }
                    });
                    if offen.is_err() {
                        return; // das Fenster ist zu
                    }
                    cx.background_executor()
                        .timer(aktualisierung::NACHSEHEN)
                        .await;
                }
            })
            .detach();
        }

        /// Der Knopf in der Seitenleiste, sobald es etwas Neues gibt.
        pub(crate) fn aktualisierung_knopf(
            &self,
            p: Palette,
            cx: &mut Context<Self>,
        ) -> Option<AnyElement> {
            self.aktualisierung.angebot()?;
            let laeuft = self.aktualisierung_laeuft;
            // Kurz halten: die Fassung steht im Fenster für die Aktualisierung und in
            // der Änderungsliste — der Knopf soll die Titelzeile nicht aufblähen.
            let text = if laeuft {
                "Wird geladen …"
            } else {
                "Aktualisieren"
            };
            Some(
                v_flex()
                    // Ohne occlude zieht die Titelleiste den Klick als
                    // Fensterziehen ab, und der Knopf tut nichts.
                    .occlude()
                    .flex_shrink_0()
                    .gap(px(4.))
                    .child(
                        Button::new("aktualisieren")
                            .primary()
                            .small()
                            .icon(IconName::Download)
                            .label(text)
                            .loading(laeuft)
                            .disabled(laeuft)
                            .on_click(cx.listener(|this, _, _, cx| this.aktualisieren_holen(cx))),
                    )
                    // Nur noch Meldungen (Fehler) — die Änderungsliste steht auf der
                    // Startseite, nicht unter dem Knopf.
                    .when_some(self.aktualisierung_meldung.clone(), |d, m| {
                        d.child(
                            div()
                                .px(px(6.))
                                .text_size(px(11.))
                                .line_height(px(15.))
                                .text_color(p.warning)
                                .child(m),
                        )
                    })
                    .into_any_element(),
            )
        }

        /// Setup laden, Pruefsumme pruefen, starten — und den Launcher beenden,
        /// damit der Installer die laufende .exe ersetzen kann.
        pub(crate) fn aktualisieren_holen(&mut self, cx: &mut Context<Self>) {
            if self.aktualisierung_laeuft {
                return;
            }
            let Some(angebot) = self.aktualisierung.angebot().cloned() else {
                return;
            };
            crate::app_log::note(&format!("aktualisieren: Klick auf {}", angebot.version));
            self.aktualisierung_laeuft = true;
            self.aktualisierung_meldung = None;
            cx.notify();
            cx.spawn(async move |this, cx| {
                let ergebnis = cx
                    .background_executor()
                    .spawn(async move {
                        let pfad = aktualisierung::laden(&angebot)?;
                        aktualisierung::einspielen(&pfad)
                    })
                    .await;
                this.update(cx, |s, cx| {
                    s.aktualisierung_laeuft = false;
                    match ergebnis {
                        Ok(()) => cx.quit(),
                        Err(e) => {
                            s.aktualisierung_meldung = Some(e);
                            cx.notify();
                        }
                    }
                })
                .ok();
            })
            .detach();
        }

        /// Kleine Zeile fuer die Fussleiste, wenn das Pruefen schiefging.
        #[allow(dead_code)]
        pub(crate) fn aktualisierung_fehler(&self) -> Option<String> {
            match &self.aktualisierung {
                Stand::Fehler(e) => Some(e.clone()),
                _ => None,
            }
        }
    }
}
