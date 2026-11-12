//! Die Lobby-Seite: Raeume fuer Ligarennen auf unserem eigenen Relay.
//!
//! Kein Konto, keine Anmeldung. Der Relay (`f1.erdi-erc.de:35501`) kennt die
//! Raeume nur daran, dass jemand Pakete mit `ERC-RELAY` und einem Raumcode
//! schickt; die Liste fragt man mit `ERC-LOBBYS` ab. Alles UDP.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Der Relay. Ueberschreibbar ueber `ERC_RELAY`, damit man ihn zum Testen
/// neben sich laufen lassen kann.
pub const RELAY: &str = "f1.erdi-erc.de:35501";
const FRIST: Duration = Duration::from_secs(3);
const ZEICHEN: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub fn relay() -> String {
    std::env::var("ERC_RELAY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| RELAY.to_owned())
}

fn ziel() -> Result<SocketAddr, String> {
    let adresse = relay();
    let mit_port = if adresse.contains(':') {
        adresse
    } else {
        format!("{adresse}:35501")
    };
    mit_port
        .to_socket_addrs()
        .map_err(|e| format!("{mit_port} liess sich nicht aufloesen: {e}"))?
        .next()
        .ok_or_else(|| format!("Zu {mit_port} fand sich keine Adresse."))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Raum {
    pub code: String,
    pub name: String,
    pub spieler: u32,
    pub kategorie: String,
}

/// Antwort des Relays in Raeume zerlegen: Kopfzeile, dann je Raum
/// `raum|name|spieler|kategorie`.
pub fn zerlegen(text: &str) -> Vec<Raum> {
    text.lines()
        .skip(1)
        .filter_map(|zeile| {
            let teile: Vec<&str> = zeile.split('|').collect();
            if teile.len() < 3 {
                return None;
            }
            Some(Raum {
                code: teile[0].trim().to_owned(),
                name: teile[1].trim().to_owned(),
                spieler: teile[2].trim().parse().unwrap_or(0),
                kategorie: teile.get(3).map(|k| k.trim().to_owned()).unwrap_or_default(),
            })
        })
        .collect()
}

/// Fragt den Relay, welche Raeume offen sind.
pub fn lobbys() -> Result<Vec<Raum>, String> {
    let ziel = ziel()?;
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("Kein Sende-Socket: {e}"))?;
    s.set_read_timeout(Some(FRIST)).ok();
    s.send_to(b"ERC-LOBBYS", ziel)
        .map_err(|e| format!("Die Anfrage ging nicht raus: {e}"))?;
    let mut puffer = [0u8; 8192];
    let (n, _) = s
        .recv_from(&mut puffer)
        .map_err(|_| "Der Relay hat nicht geantwortet.".to_string())?;
    Ok(zerlegen(&String::from_utf8_lossy(&puffer[..n])))
}

/// Ein frischer Raumcode: vier Zeichen, ohne 0/O und 1/I.
pub fn raum_neu() -> String {
    let saat = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(12_345)
        ^ ((std::process::id() as u64) << 17);
    let mut wert = saat;
    let mut aus = String::with_capacity(4);
    for _ in 0..4 {
        aus.push(ZEICHEN[(wert % ZEICHEN.len() as u64) as usize] as char);
        wert /= ZEICHEN.len() as u64;
        wert = wert.rotate_left(7) ^ saat;
    }
    aus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liest_die_liste_des_relays() {
        let text = "ERC-LOBBYS\nAB12|Samstag, Monza|3|F1\nXY99|Test|1|";
        let raeume = zerlegen(text);
        assert_eq!(raeume.len(), 2);
        assert_eq!(raeume[0].code, "AB12");
        assert_eq!(raeume[0].name, "Samstag, Monza");
        assert_eq!(raeume[0].spieler, 3);
        assert_eq!(raeume[0].kategorie, "F1");
        assert_eq!(raeume[1].kategorie, "");
    }

    #[test]
    fn eine_leere_liste_ist_leer() {
        assert!(zerlegen("ERC-LOBBYS").is_empty());
        assert!(zerlegen("").is_empty());
    }

    #[test]
    fn raumcodes_sind_vier_deutliche_zeichen() {
        for _ in 0..50 {
            let code = raum_neu();
            assert_eq!(code.chars().count(), 4);
            assert!(code.bytes().all(|b| ZEICHEN.contains(&b)), "{code}");
            assert!(!code.contains('0') && !code.contains('O') && !code.contains('1'));
        }
    }
}

// ------------------------------------------------------------ Eine Sitzung

use std::collections::{BTreeSet, HashMap};
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;

use swofty_core::baseline::Identity;
use crate::game::Install;
use swofty_core::multiplayer::bridge::{Bridge, BridgeConfig, NetplayStatus};
use swofty_core::multiplayer::fingerprint::{self, Fingerprint};
use swofty_core::catalog::Addon;
use swofty_core::multiplayer::game::{self, Category, Command};
use swofty_core::multiplayer::relay::{RelayUdp, room_bytes, room_key};
use swofty_core::multiplayer::transport::Transport;
use swofty_core::profile::Profile;
use swofty_core::store::Store;

/// Der Host ist immer Mitglied 1 — die virtuelle Adresse `10.77.0.1` im Spiel.
pub const HOST_MEMBER: u8 = 1;
/// Der Port, auf dem das Spiel zuhört, bevor es selbst einen meldet.
const GAME_PORT: u16 = 21_414;
const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(15);

/// Eine offene Lobby: die Brücke zum Spiel, der Transport über unseren Relay.
pub struct Sitzung {
    pub code: String,
    pub ich: u8,
    pub host: bool,
    pub name: String,
    pub mitglieder: BTreeSet<u8>,
    /// Was beim Angleichen getan wurde, fuer die Anzeige im Raum.
    pub anpassungen: Vec<String>,
    /// Der Mod-Abdruck dieses Rechners, fuer die Beschreibung des Raums.
    abdruck: Option<String>,
    transport: Arc<RelayUdp>,
    bridge: Bridge,
}

/// Eine Mitgliedsnummer fuer einen Gast, die nicht mit dem Host kollidiert.
fn freie_nummer() -> u8 {
    let saat = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(7);
    let n = saat ^ ((std::process::id() as u64) << 19);
    2 + (n % 253) as u8
}

/// Der Abdruck dessen, was dieses Profil spielt: Spielstand, Mods mit Fassung und
/// Quelle, und die Spieloptionen, die das Fahrverhalten angehen.
fn eigener_abdruck(
    store: &Store,
    profile: &Profile,
    install: &Install,
) -> Result<Fingerprint, String> {
    let spiel = Identity::detect(&install.dir, install.build_id)
        .map_err(|e| format!("Das Spiel ließ sich nicht lesen: {e}"))?;
    fingerprint::fingerprint(store, profile, &spiel)
        .map_err(|e| format!("Der Mod-Stand ließ sich nicht lesen: {e}"))
}

fn starten(
    spielordner: &Path,
    code: String,
    ich: u8,
    host: bool,
    name: &str,
    kategorie: Category,
    bekannte: Vec<u8>,
    transport: Arc<RelayUdp>,
    abdruck: Option<String>,
) -> Result<Sitzung, String> {
    let game_port = NetplayStatus::read(spielordner)
        .map(|s| s.game_port)
        .filter(|p| *p != 0)
        .unwrap_or(GAME_PORT);
    let peers: HashMap<u8, std::net::SocketAddr> =
        bekannte.iter().map(|&m| (m, transport.relay_addr())).collect();
    let _ = transport.set_peers(&peers);
    let bridge = Bridge::start(
        BridgeConfig {
            self_member: ich,
            members: bekannte.clone(),
            key: room_key(&code),
            game_port,
        },
        transport.clone(),
    )
    .map_err(|e| format!("Die Brücke ließ sich nicht starten: {e}"))?;

    game::send(
        spielordner,
        &Command::Configure {
            self_member: ich,
            base: bridge.base(),
            name: game::driver_name(name),
        },
        CONFIGURE_TIMEOUT,
    )
    .map_err(|e| {
        format!(
            "Das Spiel hört nicht auf die Lobby: {e}. Starte F1 25 über den Knopf              Play in diesem Fenster — nur dann horcht das Spiel auf den Raum."
        )
    })?;

    if !host && !transport.warte_auf_host(Duration::from_secs(20)) {
        return Err(
            "Der Host antwortet nicht. Laeuft bei ihm der Launcher mit offenem Raum,                      und steht F1 25 in seiner Lobby?".to_owned(),
        );
    }

    let befehl = if host {
        Command::Host { category: kategorie }
    } else {
        Command::Join {
            ip: Ipv4Addr::new(10, 77, 0, HOST_MEMBER),
        }
    };
    game::send(spielordner, &befehl, befehl.timeout())
        .map_err(|e| {
        format!(
            "Das Spiel konnte den Raum nicht öffnen: {e}. Steht es im Hauptmenü?"
        )
    })?;

    let mut mitglieder = BTreeSet::from([ich]);
    mitglieder.extend(bekannte);
    if host {
        transport.describe(name, kategorie.as_str(), abdruck.as_deref());
    }
    let sitzung = Sitzung {
        code,
        ich,
        host,
        name: name.to_owned(),
        mitglieder,
        anpassungen: Vec::new(),
        abdruck,
        transport,
        bridge,
    };
    Ok(sitzung)
}

/// Was der Gast in der Hand haelt, zwischen Angleichen und Spielstart.
pub struct Vorbereitung {
    pub code: String,
    ich: u8,
    transport: Arc<RelayUdp>,
    abdruck: Option<String>,
    pub anpassungen: Vec<String>,
}

impl Sitzung {
    /// Einen Raum aufmachen: Code ziehen, Bruecke starten, das Spiel hosten lassen.
    pub fn hosten(
        spielordner: &Path,
        store: &Store,
        profile: &Profile,
        install: &Install,
        name: &str,
        kategorie: Category,
    ) -> Result<Self, String> {
        let abdruck = eigener_abdruck(store, profile, install).ok();
        let code = raum_neu();
        let transport = Arc::new(
            RelayUdp::new(relay(), &code, HOST_MEMBER)
                .map_err(|e| format!("Kein Platz im Raum: {e}"))?,
        );
        starten(
            spielordner,
            code,
            HOST_MEMBER,
            true,
            name,
            kategorie,
            Vec::new(),
            transport,
            abdruck.map(|f| f.digest),
        )
    }

    /// Der erste Teil eines Beitritts, bevor das Spiel laeuft: Raumcode, Verbindung
    /// zum Relay, Mod-Stand des Hosts holen und angleichen. Danach steht das Profil
    /// auf dem Stand des Hosts — das Spiel muss diesen Stand beim Start lesen.
    pub fn vorbereiten(
        code: &str,
        store: &Store,
        profile: &mut Profile,
        install: &Install,
        katalog: &[Addon],
    ) -> Result<Vorbereitung, String> {
        let code = code.trim().to_uppercase();
        room_bytes(&code).map_err(|e| e.to_string())?;
        let ich = freie_nummer();
        let transport = Arc::new(
            RelayUdp::new(relay(), &code, ich).map_err(|e| format!("Kein Platz im Raum: {e}"))?,
        );
        transport.announce();
        let abdruck = transport
            .wait_for_host_abdruck(Duration::from_secs(8))
            .map_err(|_| {
                format!(
                    "Der Host hat den Raum {code} nicht beschrieben. Stimmt der Code -                      und laeuft drueben der Launcher?"
                )
            })?;

        let mut anpassungen = Vec::new();
        let roh = transport
            .hole_abdruck(Duration::from_secs(15))
            .map_err(|e| format!("Der Mod-Stand des Hosts kam nicht an: {e}"))?;
        if let Some(roh) = roh {
            let host: Fingerprint = serde_json::from_slice(&roh)
                .map_err(|e| format!("Der Mod-Stand des Hosts ist unlesbar: {e}"))?;
            let ergebnis = crate::abgleich::angleichen(store, profile, install, &host, katalog)?;
            if !ergebnis.getan.is_empty() {
                store
                    .save_profile(profile)
                    .map_err(|e| format!("Das Profil liess sich nicht sichern: {e}"))?;
            }
            if !ergebnis.gelungen() {
                return Err(format!(
                    "Die Mods passen noch nicht:

{}

Der Beitritt unterbleibt,                      sonst stuerzt die Lobby beim Start ab.",
                    ergebnis.offen.join("
")
                ));
            }
            anpassungen = ergebnis.getan;
        }
        Ok(Vorbereitung {
            code,
            ich,
            transport,
            abdruck,
            anpassungen,
        })
    }

    /// Der zweite Teil: das Spiel konfigurieren und beitreten. Laeuft, wenn das Spiel
    /// im Hauptmenue steht.
    pub fn beitreten(
        vorbereitung: Vorbereitung,
        spielordner: &Path,
        name: &str,
    ) -> Result<Self, String> {
        let mut sitzung = starten(
            spielordner,
            vorbereitung.code,
            vorbereitung.ich,
            false,
            name,
            Category::F12026,
            vec![HOST_MEMBER],
            vorbereitung.transport,
            vorbereitung.abdruck,
        )?;
        sitzung.anpassungen = vorbereitung.anpassungen;
        Ok(sitzung)
    }

    /// Wer seit dem letzten Blick dazugekommen ist, bekommt seinen Brückenport.
    pub fn neue_mitglieder(&mut self) -> Vec<u8> {
        let mut neu = Vec::new();
        for m in self.transport.seen() {
            if self.mitglieder.insert(m) {
                neu.push(m);
            }
        }
        if !neu.is_empty() {
            let peers: HashMap<u8, std::net::SocketAddr> = self
                .mitglieder
                .iter()
                .filter(|&&m| m != self.ich)
                .map(|&m| (m, self.transport.relay_addr()))
                .collect();
            let _ = self.transport.set_peers(&peers);
            for &m in &neu {
                let _ = self.bridge.add_member(m);
            }
            if self.host {
                // Die Beschreibung frisch halten: der Relay vergisst sie mit dem Raum.
                self.transport.describe(&self.name, "f1-2026", self.abdruck.as_deref());
            }
        }
        neu
    }

    /// Raum verlassen und dem Spiel sagen, dass es vorbei ist.
    pub fn verlassen(&mut self, spielordner: &Path) {
        let _ = game::send(spielordner, &Command::Leave, Duration::from_secs(3));
        // Die Brücke haengt am Drop; hier bleibt nichts offen.
    }
}


// ------------------------------------------------------------------ Fenster
//
// In einem eigenen Modul, damit die schweren Typen von gpui_kit nicht in den
// Bereich der Tests oben hineinragen — sonst sprengt die Expansion von
// `#[test]` das Rekursionslimit des Compilers.
mod view {
    use super::*;

    use std::time::Duration;

    use gpui_kit::assets::IconName;
    use gpui_kit::component::button::{Button, ButtonVariants};
    use gpui_kit::component::input::{Input, InputEvent, InputState};
    use gpui_kit::component::{Disableable, Sizable, h_flex, v_flex};
    use gpui_kit::prelude::FluentBuilder;
    use gpui_kit::*;

    use crate::pages::GUTTER;
    use crate::shell::{Page, Shell};
    use crate::theme::Palette;

    const TICK: Duration = Duration::from_secs(1);

    pub(crate) struct Lobby {
        pub(crate) raeume: Vec<Raum>,
        pub(crate) fehler: Option<String>,
        pub(crate) laedt: bool,
        pub(crate) geladen: bool,
        pub(crate) sitzung: Option<Sitzung>,
        pub(crate) arbeitet: bool,
        pub(crate) meldung: Option<String>,
        /// Welche Fahrzeugklasse der Raum faehrt. Muss auf beiden Rechnern
        /// vorhanden sein, sonst lehnt das Spiel den Beitritt ab.
        pub(crate) kategorie: Category,
        name: Entity<InputState>,
        code: Entity<InputState>,
        _inputs: [Subscription; 2],
    }

    impl Lobby {
        pub(crate) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
            let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name der Lobby"));
            let code = cx.new(|cx| InputState::new(window, cx).placeholder("Raumcode"));
            let changed = |cx: &mut Context<Shell>, input: &Entity<InputState>| {
                cx.subscribe(input, |_, _, e: &InputEvent, cx| {
                    if matches!(e, InputEvent::Change) {
                        cx.notify();
                    }
                })
            };
            let inputs = [changed(cx, &name), changed(cx, &code)];
            Self {
                raeume: Vec::new(),
                fehler: None,
                laedt: false,
                geladen: false,
                sitzung: None,
                arbeitet: false,
                meldung: None,
                kategorie: Category::F12026,
                name,
                code,
                _inputs: inputs,
            }
        }

        fn beschaeftigt(&self) -> bool {
            self.arbeitet || self.sitzung.is_some()
        }
    }

    impl Shell {
        /// Holt die offenen Raeume im Hintergrund; die Oberflaeche wartet nicht.
        pub(crate) fn fetch_lobbys(&mut self, cx: &mut Context<Self>) {
            if self.lobby.laedt {
                return;
            }
            self.lobby.laedt = true;
            cx.spawn(async move |this, cx| {
                let ergebnis = cx.background_executor().spawn(async { lobbys() }).await;
                this.update(cx, |s, cx| {
                    s.lobby.laedt = false;
                    s.lobby.geladen = true;
                    match ergebnis {
                        Ok(raeume) => {
                            s.lobby.raeume = raeume;
                            s.lobby.fehler = None;
                        }
                        Err(e) => {
                            s.lobby.raeume.clear();
                            s.lobby.fehler = Some(e);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }

        /// Haelt die Bruecke am Laufen: wer dazukommt, bekommt seinen Port.
        pub(crate) fn watch_lobby(window: &mut Window, cx: &mut Context<Self>) {
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor().timer(TICK).await;
                    let ok = this.update_in(cx, |s, _, cx| {
                        let neu = s
                            .lobby
                            .sitzung
                            .as_mut()
                            .map(Sitzung::neue_mitglieder)
                            .unwrap_or_default();
                        if !neu.is_empty() {
                            cx.notify();
                        }
                    });
                    if ok.is_err() {
                        break;
                    }
                }
            })
            .detach();
        }

        /// Raum aufmachen. Laeuft im Hintergrund, weil das Spiel antworten muss.
        pub(crate) fn lobby_hosten(&mut self, cx: &mut Context<Self>) {
            if self.lobby.beschaeftigt() {
                return;
            }
            let Some(spielordner) = self.spielordner_fuer_lobby() else {
                self.lobby.meldung = Some("F1 25 wurde nicht gefunden.".to_owned());
                cx.notify();
                return;
            };
            let (Some(install), store, profile) = (
                self.install_fuer_lobby(),
                self.store.clone(),
                self.library.profile.clone(),
            ) else {
                self.lobby.meldung = Some("F1 25 wurde nicht gefunden.".to_owned());
                cx.notify();
                return;
            };
            let kategorie = self.lobby.kategorie;
            let name = self.lobby.name.read(cx).value().trim().to_owned();
            let name = if name.is_empty() {
                "ERC-Lobby".to_owned()
            } else {
                name
            };
            self.lobby.arbeitet = true;
            self.lobby.meldung = None;
            cx.spawn(async move |this, cx| {
                let ergebnis = cx
                    .background_executor()
                    .spawn(async move {
                        Sitzung::hosten(std::path::Path::new(&spielordner), &store, &profile, &install, &name, kategorie)
                    })
                    .await;
                this.update(cx, |s, cx| {
                    s.lobby.arbeitet = false;
                    match ergebnis {
                        Ok(sitzung) => {
                            s.lobby.sitzung = Some(sitzung);
                            s.lobby.meldung = None;
                            s.fetch_lobbys(cx);
                        }
                        Err(e) => s.lobby.meldung = Some(e),
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }

        /// Einem Raum beitreten. Ist das Spiel zu, werden erst die Mods angeglichen,
        /// dann wird F1 25 gestartet, auf das Hauptmenue gewartet und beigetreten.
        pub(crate) fn lobby_beitreten(&mut self, cx: &mut Context<Self>) {
            if self.lobby.beschaeftigt() {
                return;
            }
            let code = self.lobby.code.read(cx).value().trim().to_owned();
            if code.is_empty() {
                return;
            }
            let Some(spielordner) = self.spielordner_fuer_lobby() else {
                self.lobby.meldung = Some("F1 25 wurde nicht gefunden.".to_owned());
                cx.notify();
                return;
            };
            let (Some(install), store, mut profile, katalog) = (
                self.install_fuer_lobby(),
                self.store.clone(),
                self.library.profile.clone(),
                self.browser.addons.clone(),
            ) else {
                self.lobby.meldung = Some("F1 25 wurde nicht gefunden.".to_owned());
                cx.notify();
                return;
            };
            if katalog.is_empty() {
                self.lobby.meldung = Some(
                    "Der Katalog ist noch nicht geladen - ohne ihn lassen sich fehlende Mods                      nicht holen. Bitte kurz warten und noch einmal versuchen."
                        .to_owned(),
                );
                cx.notify();
                return;
            }
            let name = self.lobby.name.read(cx).value().trim().to_owned();
            let spiel_laeuft = !self.processes.is_empty();
            self.lobby.arbeitet = true;
            self.lobby.meldung = Some("Mods werden angeglichen ...".to_owned());
            cx.notify();
            cx.spawn(async move |this, cx| {
                // 1) Angleichen, ohne dass das Spiel laeuft.
                let ordner = spielordner.clone();
                let vor = cx
                    .background_executor()
                    .spawn(async move {
                        Sitzung::vorbereiten(&code, &store, &mut profile, &install, &katalog)
                    })
                    .await;
                let vor = match vor {
                    Ok(v) => v,
                    Err(e) => {
                        this.update(cx, |s, cx| {
                            s.lobby.arbeitet = false;
                            s.lobby.meldung = Some(e);
                            cx.notify();
                        })
                        .ok();
                        return;
                    }
                };
                let geaendert = !vor.anpassungen.is_empty();

                // 2) Laeuft das Spiel schon mit dem alten Stand, hilft nur ein Neustart.
                if spiel_laeuft {
                    if geaendert {
                        this.update(cx, |s, cx| {
                            s.lobby.arbeitet = false;
                            s.reload_in_background(cx);
                            s.lobby.meldung = Some(
                                "Deine Mods wurden an den Host angeglichen. F1 25 laeuft aber                      schon mit dem alten Stand: beende das Spiel, starte es ueber Play neu und                      tritt dann noch einmal bei."
                                    .to_owned(),
                            );
                            cx.notify();
                        })
                        .ok();
                        return;
                    }
                } else {
                    // 3) Spiel starten und aufs Hauptmenue warten.
                    let gestartet = this.update(cx, |s, cx| {
                        if geaendert {
                            s.reload_in_background(cx);
                        }
                        let ok = s.spiel_fuer_lobby_starten(cx);
                        if let Err(e) = &ok {
                            s.lobby.arbeitet = false;
                            s.lobby.meldung = Some(e.clone());
                        } else {
                            s.lobby.meldung = Some("F1 25 startet ...".to_owned());
                        }
                        cx.notify();
                        ok.is_ok()
                    });
                    if !matches!(gestartet, Ok(true)) {
                        return;
                    }
                    let bis = std::time::Instant::now() + std::time::Duration::from_secs(420);
                    let bereit = cx
                        .background_executor()
                        .spawn(async move { warte_auf_hauptmenue(Path::new(&ordner), bis) })
                        .await;
                    if !bereit {
                        this.update(cx, |s, cx| {
                            s.lobby.arbeitet = false;
                            s.lobby.meldung = Some(
                                "F1 25 ist nicht bis ins Hauptmenue gekommen. Sieh in der                                  Konsole nach, was zuletzt passiert ist."
                                    .to_owned(),
                            );
                            cx.notify();
                        })
                        .ok();
                        return;
                    }
                }

                // 4) Beitreten.
                let ordner = spielordner.clone();
                let ergebnis = cx
                    .background_executor()
                    .spawn(async move { Sitzung::beitreten(vor, Path::new(&ordner), &name) })
                    .await;
                this.update(cx, |s, cx| {
                    s.lobby.arbeitet = false;
                    match ergebnis {
                        Ok(sitzung) => {
                            s.lobby.sitzung = Some(sitzung);
                            s.lobby.meldung = None;
                            s.fetch_lobbys(cx);
                        }
                        Err(e) => s.lobby.meldung = Some(e),
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }

        pub(crate) fn lobby_verlassen(&mut self, cx: &mut Context<Self>) {
            if let Some(mut sitzung) = self.lobby.sitzung.take() {
                if let Some(ordner) = self.spielordner_fuer_lobby() {
                    sitzung.verlassen(std::path::Path::new(&ordner));
                }
                self.fetch_lobbys(cx);
            }
            self.lobby.meldung = None;
            cx.notify();
        }

        pub(crate) fn lobby_page(&mut self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
            if !self.lobby.geladen && !self.lobby.laedt {
                self.fetch_lobbys(cx);
            }
            let zustand = if self.lobby.arbeitet {
                "Einen Moment …".to_owned()
            } else if let Some(s) = &self.lobby.sitzung {
                if s.host {
                    format!("Du hostest — Raum {}", s.code)
                } else {
                    format!("Du bist im Raum {}", s.code)
                }
            } else if self.lobby.laedt {
                "Wird abgefragt …".to_owned()
            } else if self.lobby.fehler.is_some() {
                "Kein Kontakt zum Server".to_owned()
            } else if self.lobby.raeume.is_empty() {
                "Keine Lobby offen".to_owned()
            } else {
                match self.lobby.raeume.len() {
                    1 => "1 Lobby offen".to_owned(),
                    n => format!("{n} Lobbys offen"),
                }
            };
            let zuruecksetzen = Button::new("lobby-refresh")
                .ghost()
                .small()
                .icon(IconName::RefreshCw)
                .label("Aktualisieren")
                .disabled(self.lobby.laedt)
                .on_click(cx.listener(|this, _, _, cx| this.fetch_lobbys(cx)));

            let inhalt = if self.lobby.sitzung.is_some() {
                self.lobby_im_raum(p, cx)
            } else {
                self.lobby_draussen(p, cx)
            };

            v_flex()
                .size_full()
                .child(self.header(Page::Multiplayer.label(), zustand, zuruecksetzen, p))
                .child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .px(px(GUTTER))
                        .pt(px(4.))
                        .gap(px(16.))
                        .child(inhalt),
                )
                .into_any_element()
        }

        fn lobby_draussen(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
            let macht = self.lobby.arbeitet;
            let erzeugen = Button::new("lobby-host")
                .primary()
                .small()
                .icon(IconName::Users)
                .label(if macht { "Öffnet …" } else { "Lobby erstellen" })
                .loading(macht)
                .disabled(macht)
                .on_click(cx.listener(|this, _, _, cx| this.lobby_hosten(cx)));
            let beitreten = Button::new("lobby-join")
                .outline()
                .small()
                .label("Beitreten")
                .loading(macht)
                .disabled(macht)
                .on_click(cx.listener(|this, _, _, cx| this.lobby_beitreten(cx)));
            let meldung = self.lobby.meldung.clone();

            v_flex()
                .gap(px(16.))
                .child(
                    v_flex()
                        .gap(px(8.))
                        .p(px(16.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(p.border_variant)
                        .bg(p.surface)
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child("Einen Raum aufmachen"),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(p.text_muted)
                                .child("Der Raum bekommt einen Code. Wer ihn hat, kann beitreten — ohne Konto, ohne offenen Port."),
                        )
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .w(px(220.))
                                        .child(Input::new(&self.lobby.name).small().disabled(macht)),
                                )
                                .child(erzeugen),
                        )
                        .child(
                            h_flex()
                                .gap(px(6.))
                                .items_center()
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(p.text_muted)
                                        .child("Fahrzeugklasse:"),
                                )
                                .child(
                                    Button::new("lobby-kat-26")
                                        .when(self.lobby.kategorie == Category::F12026, |b| b.primary())
                                        .when(self.lobby.kategorie != Category::F12026, |b| b.outline())
                                        .small()
                                        .label("F1 26")
                                        .disabled(macht)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.lobby.kategorie = Category::F12026;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("lobby-kat-25")
                                        .when(self.lobby.kategorie == Category::F12025, |b| b.primary())
                                        .when(self.lobby.kategorie != Category::F12025, |b| b.outline())
                                        .small()
                                        .label("F1 25")
                                        .disabled(macht)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.lobby.kategorie = Category::F12025;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .child(
                    v_flex()
                        .gap(px(8.))
                        .p(px(16.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(p.border_variant)
                        .bg(p.surface)
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child("Einem Raum beitreten"),
                        )
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .w(px(220.))
                                        .child(Input::new(&self.lobby.code).small().disabled(macht)),
                                )
                                .child(beitreten),
                        ),
                )
                .when_some(meldung, |d, text| {
                    d.child(div().text_size(px(13.)).text_color(p.warning).child(text))
                })
                .children(self.lobby_liste(p))
                .into_any_element()
        }

        fn lobby_im_raum(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
            let Some(sitzung) = &self.lobby.sitzung else {
                return div().into_any_element();
            };
            let raus = Button::new("lobby-leave")
                .outline()
                .small()
                .label("Raum verlassen")
                .on_click(cx.listener(|this, _, _, cx| this.lobby_verlassen(cx)));
            v_flex()
                .gap(px(14.))
                .child(
                    v_flex()
                        .gap(px(6.))
                        .p(px(20.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(p.border_variant)
                        .bg(p.surface)
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(p.text_placeholder)
                                .child("RAUMCODE"),
                        )
                        .child(
                            div()
                                .text_size(px(34.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(p.accent_hover)
                                .child(sitzung.code.clone()),
                        )
                        .children(sitzung.anpassungen.iter().map(|zeile| {
                            div()
                                .text_size(px(12.))
                                .text_color(p.warning)
                                .child(format!("• {zeile}"))
                        }))
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(p.text_muted)
                                .child(if sitzung.host {
                                    "Gib diesen Code weiter. Die anderen tippen ihn bei sich ein."
                                } else {
                                    "Du bist beigetreten. Zurück in der Lobby das Spiel starten."
                                }),
                        ),
                )
                .child(
                    h_flex()
                        .gap(px(16.))
                        .items_center()
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(p.text_muted)
                                .child(format!("{} Mitglieder", sitzung.mitglieder.len())),
                        )
                        .child(raus),
                )
                .into_any_element()
        }

        fn lobby_liste(&self, p: Palette) -> Option<AnyElement> {
            if let Some(fehler) = &self.lobby.fehler {
                return Some(
                    v_flex()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(p.text)
                                .child("Der Server antwortet nicht."),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(p.text_muted)
                                .child(fehler.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(p.text_placeholder)
                                .child(format!("Adresse: {}", relay())),
                        )
                        .into_any_element(),
                );
            }
            if self.lobby.raeume.is_empty() {
                return Some(
                    v_flex()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(p.text)
                                .child("Gerade ist keine Lobby offen."),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(p.text_muted)
                                .child("Mach die erste auf — der Code genügt zum Beitreten."),
                        )
                        .into_any_element(),
                );
            }
            Some(
                v_flex()
                    .gap(px(8.))
                    .children(self.lobby.raeume.iter().map(|raum| raum_karte(raum, p)))
                    .into_any_element(),
            )
        }

        /// Die Spielkopie, deren Mod-Stand in den Abdruck geht.
        /// F1 25 so starten, wie es der Beitritt braucht: mit armiertem Netplay.
        fn spiel_fuer_lobby_starten(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
            let install = self
                .install_fuer_lobby()
                .ok_or_else(|| "F1 25 wurde nicht gefunden.".to_owned())?;
            let launch = crate::game::Launch::start(
                &self.store,
                &self.library.profile.name,
                &install,
                self.launch_mode(),
                Some(swofty_core::multiplayer::bridge::ARMED),
                None,
                None,
                false,
            )?;
            self.launch = Some(launch);
            let _ = cx;
            Ok(())
        }

        fn install_fuer_lobby(&self) -> Option<Install> {
            self.install.clone().or_else(|| self.copies.first().cloned())
        }

        /// Der Spielordner, in dem das Spiel auf Netplay-Befehle hoert.
        fn spielordner_fuer_lobby(&self) -> Option<String> {
            self.install
                .as_ref()
                .map(|i| i.dir.clone())
                .or_else(|| self.copies.first().map(|c| c.dir.clone()))
                .map(|p| p.to_string_lossy().into_owned())
        }
    }

    /// Wartet, bis das Spiel im Hauptmenue steht und auf Befehle hoert.
    fn warte_auf_hauptmenue(ordner: &std::path::Path, bis: std::time::Instant) -> bool {
        use swofty_core::multiplayer::game::{self, Command, Screen};
        while std::time::Instant::now() < bis {
            if let Ok(reply) = game::send(ordner, &Command::Status, Duration::from_secs(2))
                && reply.status.screen == Screen::MainMenu
            {
                return true;
            }
            std::thread::sleep(Duration::from_secs(3));
        }
        false
    }

    fn raum_karte(raum: &Raum, p: Palette) -> AnyElement {
        h_flex()
            .items_center()
            .gap(px(14.))
            .px(px(16.))
            .py(px(12.))
            .rounded(px(10.))
            .border_1()
            .border_color(p.border_variant)
            .bg(p.surface)
            .child(
                div()
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .bg(p.accent.opacity(0.18))
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.accent_hover)
                    .child(raum.code.clone()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(raum.name.clone()),
                    )
                    .when(!raum.kategorie.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(p.text_placeholder)
                                .child(raum.kategorie.clone()),
                        )
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .text_size(px(13.))
                    .text_color(p.text_muted)
                    .child(format!("{} Fahrer", raum.spieler)),
            )
            .into_any_element()
    }
}

pub(crate) use view::Lobby;
