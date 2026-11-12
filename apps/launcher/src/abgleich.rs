//! Gleicht das eigene Profil an den Mod-Stand des Hosts an, bevor beigetreten wird.
//!
//! Der Host schickt seinen Abdruck (siehe `swofty_core::multiplayer::fingerprint`).
//! Daraus folgt, was zu tun ist: fehlende oder falsch gefasste Mods werden aus dem
//! Katalog geholt, eigene Mods, die der Host nicht fährt, werden abgeschaltet.
//! Was danach noch abweicht, steht als offener Punkt im Ergebnis — der Beitritt
//! unterbleibt dann.

use std::collections::BTreeSet;

use semver::Version;
use swofty_core::baseline::Identity;
use swofty_core::catalog::{self, Addon};
use swofty_core::multiplayer::fingerprint::{self, Fingerprint};
use swofty_core::profile::Profile;
use swofty_core::store::Store;

use crate::game::Install;

/// Was getan wurde, und was offen blieb.
#[derive(Debug, Default)]
pub struct Ergebnis {
    pub getan: Vec<String>,
    pub offen: Vec<String>,
}

impl Ergebnis {
    pub fn gelungen(&self) -> bool {
        self.offen.is_empty()
    }
}

fn abdruck(
    store: &Store,
    profile: &Profile,
    install: &Install,
) -> Result<Fingerprint, String> {
    let spiel = Identity::detect(&install.dir, install.build_id)
        .map_err(|e| format!("Das Spiel ließ sich nicht lesen: {e}"))?;
    fingerprint::fingerprint(store, profile, &spiel)
        .map_err(|e| format!("Der eigene Mod-Stand ließ sich nicht lesen: {e}"))
}

/// Das Verzeichnis der Fassungen einer Katalog-Mod, die der Host fährt.
fn host_fassung<'a>(addon: &'a Addon, version: &Version) -> Option<&'a catalog::Version> {
    addon
        .versions
        .iter()
        .find(|v| &v.version == version)
        .or_else(|| addon.versions.iter().max_by(|a, b| a.version.cmp(&b.version)))
}

/// Gleicht `profile` an `host` an. Der Aufrufer muss das Profil danach sichern und
/// die Bibliothek neu laden.
pub fn angleichen(
    store: &Store,
    profile: &mut Profile,
    install: &Install,
    host: &Fingerprint,
    katalog: &[Addon],
) -> Result<Ergebnis, String> {
    let mut ergebnis = Ergebnis::default();
    let eigen = abdruck(store, profile, install)?;

    let host_ids: BTreeSet<&str> = host.addons.iter().map(|a| a.id.as_str()).collect();
    let eigen_ids: BTreeSet<&str> = eigen.addons.iter().map(|a| a.id.as_str()).collect();

    // 1) Was der Host fährt und wir nicht (oder in anderer Fassung): aus dem Katalog holen.
    for ihre in &host.addons {
        if let Some(unsere) = eigen.addons.iter().find(|a| a.id == ihre.id) {
            if unsere.version == ihre.version {
                continue;
            }
        }
        let Some(addon) = katalog.iter().find(|a| a.id == ihre.id) else {
            ergebnis.offen.push(format!(
                "{} {} fehlt im Katalog — bitte von Hand installieren",
                ihre.name, ihre.version
            ));
            continue;
        };
        let Some(fassung) = host_fassung(addon, &ihre.version) else {
            ergebnis.offen.push(format!(
                "{} {} ist im Katalog nicht mehr zu haben",
                ihre.name, ihre.version
            ));
            continue;
        };
        match catalog::requires::install(store, katalog, addon, fassung, &|_| {}) {
            Ok(fertig) => {
                for m in &fertig {
                    ergebnis
                        .getan
                        .push(format!("{} {} geholt", m.name, m.version));
                }
            }
            Err(e) => ergebnis
                .offen
                .push(format!("{} {} ließ sich nicht holen: {e}", ihre.name, ihre.version)),
        }
    }

    // 2) Schalter stellen: was der Host fährt an, was er nicht fährt aus.
    for ihre in &host.addons {
        if eigen_ids.contains(ihre.id.as_str()) {
            continue;
        }
        match profile.set_enabled(&ihre.id, true) {
            Ok(()) => ergebnis
                .getan
                .push(format!("{} eingeschaltet", ihre.name)),
            Err(e) => ergebnis
                .offen
                .push(format!("{} ließ sich nicht einschalten: {e}", ihre.name)),
        }
    }
    for unsere in &eigen.addons {
        if host_ids.contains(unsere.id.as_str()) {
            continue;
        }
        match profile.set_enabled(&unsere.id, false) {
            Ok(()) => ergebnis
                .getan
                .push(format!("{} ausgeschaltet", unsere.name)),
            Err(e) => ergebnis
                .offen
                .push(format!("{} ließ sich nicht ausschalten: {e}", unsere.name)),
        }
    }

    // 3) Nachsehen, ob es jetzt passt.
    let danach = abdruck(store, profile, install)?;
    for m in fingerprint::compare(&danach, host, katalog) {
        ergebnis.offen.push(m.line());
    }
    Ok(ergebnis)
}
