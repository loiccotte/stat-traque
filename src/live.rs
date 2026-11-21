//! API du jeu en cours (port 2999, sans mot de passe, ouverte par le jeu lui-même).
//!
//! Optimisation : ce thread n'existe QUE pendant une partie. Il est créé quand la
//! phase passe à "InProgress" et s'arrête dès qu'elle change. Hors partie : rien.
//! On ne garde pas le gros JSON en mémoire : on en extrait un petit résumé.
//! On lit `playerlist` + `gamestats` (petits) plutôt que `allgamedata`, qui contient
//! aussi la liste des événements de la partie et grossit au fil des minutes.

use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::evenement::Evenement;

/// Toutes les combien de secondes on relit la partie.
const INTERVALLE: Duration = Duration::from_secs(5);
/// Après ce nombre d'échecs d'affilée (~1 min), la partie est finie : le thread s'arrête seul.
/// (Le client LoL peut être fermé pendant la partie : on ne compte pas sur lui.)
const ECHECS_MAX: u32 = 12;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct JoueurLive {
    pub pseudo: Box<str>,
    pub champion: Box<str>,
    /// true = équipe bleue (ORDER), false = rouge (CHAOS)
    pub bleu: bool,
    pub niveau: u8,
    pub kills: u16,
    pub morts: u16,
    pub assists: u16,
    pub cs: u16,
    /// Identifiants des objets (u32 : certains ont 6 chiffres, ex. objets d'Ornn).
    pub items: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstantanePartie {
    pub mode: Box<str>,
    pub secondes: u32,
    pub joueurs: Vec<JoueurLive>,
}

impl InstantanePartie {
    /// Extrait le résumé depuis `/liveclientdata/playerlist` et `/liveclientdata/gamestats`.
    pub fn depuis_json(joueurs: &Value, stats: &Value) -> Option<Self> {
        let texte = |x: &Value| Box::<str>::from(x.as_str().unwrap_or("?"));
        let nombre = |x: &Value| x.as_u64().unwrap_or(0) as u16;
        let joueurs = joueurs
            .as_array()?
            .iter()
            .map(|j| JoueurLive {
                pseudo: texte(&j["riotIdGameName"]),
                champion: texte(&j["championName"]),
                bleu: j["team"] == "ORDER",
                niveau: j["level"].as_u64().unwrap_or(0) as u8,
                kills: nombre(&j["scores"]["kills"]),
                morts: nombre(&j["scores"]["deaths"]),
                assists: nombre(&j["scores"]["assists"]),
                cs: nombre(&j["scores"]["creepScore"]),
                items: j["items"]
                    .as_array()
                    .map(|l| l.iter().filter_map(|i| i["itemID"].as_u64()).map(|i| i as u32).collect())
                    .unwrap_or_default(),
            })
            .collect();
        Some(Self {
            mode: texte(&stats["gameMode"]),
            secondes: stats["gameTime"].as_f64().unwrap_or(0.0) as u32,
            joueurs,
        })
    }
}

/// Poignée sur le thread de partie : quand on la lâche (`drop`), le thread s'arrête.
pub struct SuiviPartie {
    actif: Arc<AtomicBool>,
}

impl SuiviPartie {
    /// Le thread tourne-t-il encore ? (il s'arrête seul quand la partie est finie)
    pub fn vivant(&self) -> bool {
        self.actif.load(Ordering::Relaxed)
    }
}

impl Drop for SuiviPartie {
    fn drop(&mut self) {
        self.actif.store(false, Ordering::Relaxed);
    }
}

/// Démarre le suivi de la partie en cours.
pub fn suivre(tx: Sender<Evenement>) -> SuiviPartie {
    let actif = Arc::new(AtomicBool::new(true));
    let drapeau = actif.clone();
    thread::Builder::new()
        .name("partie".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            // On ne garde qu'une empreinte de l'état précédent (8 octets, pas de copie).
            let mut empreinte_precedente = 0u64;
            let mut echecs = 0;
            while drapeau.load(Ordering::Relaxed) {
                match lire_partie() {
                    Some(p) => {
                        echecs = 0;
                        // On n'envoie que si quelque chose a changé (KDA, items, niveaux...).
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        p.joueurs.hash(&mut h);
                        let empreinte = h.finish();
                        if empreinte != empreinte_precedente {
                            empreinte_precedente = empreinte;
                            if tx.send(Evenement::Partie(p)).is_err() {
                                break;
                            }
                        }
                    }
                    None => {
                        echecs += 1;
                        if echecs >= ECHECS_MAX {
                            break;
                        }
                    }
                }
                thread::sleep(INTERVALLE);
            }
            drapeau.store(false, Ordering::Relaxed);
        })
        .expect("impossible de lancer le thread de partie");
    SuiviPartie { actif }
}

fn lire(chemin: &str) -> Option<Value> {
    crate::http::local()
        .get(&format!("https://127.0.0.1:2999/liveclientdata/{chemin}"))
        .call()
        .ok()?
        .into_json()
        .ok()
}

fn lire_partie() -> Option<InstantanePartie> {
    let joueurs = lire("playerlist")?;
    let stats = lire("gamestats")?;
    if crate::enregistreur::actif() {
        crate::enregistreur::noter(serde_json::json!({"src": "live", "joueurs": joueurs, "stats": stats}));
    }
    InstantanePartie::depuis_json(&joueurs, &stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_une_partie() {
        let joueurs: Value = serde_json::from_str(
            r#"[{"riotIdGameName":"Faker","championName":"Ahri","team":"ORDER","level":9,
                 "scores":{"kills":3,"deaths":1,"assists":4,"creepScore":102},
                 "items":[{"itemID":3020},{"itemID":226655}]}]"#,
        )
        .unwrap();
        let stats: Value = serde_json::from_str(r#"{"gameMode":"CLASSIC","gameTime":754.3}"#).unwrap();
        let p = InstantanePartie::depuis_json(&joueurs, &stats).unwrap();
        assert_eq!(p.secondes, 754);
        assert_eq!(&*p.mode, "CLASSIC");
        let j = &p.joueurs[0];
        assert!(j.bleu);
        assert_eq!((j.kills, j.morts, j.assists, j.cs, j.niveau), (3, 1, 4, 102, 9));
        assert_eq!(j.items, vec![3020, 226655]); // id à 6 chiffres conservé
    }
}
