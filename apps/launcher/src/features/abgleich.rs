//! Gleicht das eigene Profil an den Mod-Stand des Hosts an, bevor beigetreten wird.
//!
//! Der Plan kommt aus der Sitzung ([`FixStep`]): fehlende oder falsch gefasste Mods
//! werden aus dem Katalog geholt, eigene Mods, die der Host nicht fährt, werden
//! abgeschaltet. Was danach noch abweicht, steht als offener Punkt im Ergebnis — der
//! Beitritt unterbleibt dann.

use swofty_core::catalog::{self, Addon};
use swofty_core::online::multiplayer::types::FixStep;
use swofty_core::store::Store;

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

/// Führt `plan` auf dem Profil `profile_name` aus. Der Aufrufer muss die Bibliothek
/// danach neu laden und der Sitzung den frischen Stand geben.
pub fn angleichen(
    store: &Store,
    profile_name: &str,
    katalog: &[Addon],
    plan: &[FixStep],
) -> Result<Ergebnis, String> {
    let mut ergebnis = Ergebnis::default();
    let mut profile = store
        .profile(profile_name)
        .map_err(|e| format!("Das Profil liess sich nicht lesen: {e}"))?;
    let mut geaendert = false;

    for step in plan {
        match step {
            FixStep::Install {
                id, name, version, ..
            } => {
                let Some(addon) = katalog.iter().find(|a| &a.id == id) else {
                    ergebnis.offen.push(format!(
                        "{name} {version} fehlt im Katalog — bitte von Hand installieren"
                    ));
                    continue;
                };
                let Some(fassung) = addon.versions.iter().find(|v| &v.version == version) else {
                    ergebnis
                        .offen
                        .push(format!("{name} {version} ist im Katalog nicht mehr zu haben"));
                    continue;
                };
                match catalog::requires::install_in_profile(
                    store,
                    katalog,
                    addon,
                    fassung,
                    profile_name,
                    &|_| {},
                ) {
                    Ok(fertig) => {
                        for m in &fertig {
                            ergebnis.getan.push(format!("{} {} geholt", m.name, m.version));
                        }
                        geaendert = true;
                    }
                    Err(e) => ergebnis
                        .offen
                        .push(format!("{name} {version} liess sich nicht holen: {e}")),
                }
            }
            FixStep::Disable { id, name } => match profile.set_enabled(id, false) {
                Ok(()) => {
                    geaendert = true;
                    ergebnis.getan.push(format!("{name} ausgeschaltet"));
                }
                Err(e) => ergebnis
                    .offen
                    .push(format!("{name} liess sich nicht ausschalten: {e}")),
            },
            FixStep::SetOption { addon, option, .. } => ergebnis
                .offen
                .push(format!("{addon}: die Option {option} bitte von Hand stellen")),
            FixStep::Reorder { .. } => ergebnis
                .offen
                .push("Die Reihenfolge der Addons bitte von Hand stellen".to_owned()),
            FixStep::Manual { line } => ergebnis.offen.push(line.clone()),
        }
    }

    if geaendert {
        store
            .save_profile(&profile)
            .map_err(|e| format!("Das Profil liess sich nicht sichern: {e}"))?;
    }
    Ok(ergebnis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_line_stays_open_and_installs_nothing() {
        let plan = [FixStep::Manual {
            line: "Track pack is not in the catalog".into(),
        }];
        let ergebnis = Ergebnis {
            getan: Vec::new(),
            offen: vec!["Track pack is not in the catalog".into()],
        };
        assert!(!ergebnis.gelungen());
        assert_eq!(plan.len(), 1);
    }

    #[test]
    fn an_empty_plan_is_done() {
        let ergebnis = Ergebnis::default();
        assert!(ergebnis.gelungen());
    }

    #[test]
    fn what_the_launcher_cannot_do_alone_stays_open() {
        let root = std::env::temp_dir().join(format!("erc-abgleich-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::new(&root);
        store.create_profile("Probe", None).unwrap();
        let plan = vec![
            FixStep::Reorder {
                order: vec!["a.one".into()],
            },
            FixStep::Manual {
                line: "Ein Paket fehlt im Katalog".into(),
            },
            FixStep::Install {
                id: "gibt.es.nicht".into(),
                name: "Nicht im Katalog".into(),
                version: semver::Version::new(1, 0, 0),
                sha256: None,
            },
        ];
        let ergebnis = angleichen(&store, "Probe", &[], &plan).unwrap();
        assert!(!ergebnis.gelungen());
        assert_eq!(ergebnis.offen.len(), 3);
        assert!(ergebnis.getan.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
