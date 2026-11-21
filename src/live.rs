//! API du jeu en cours (port 2999, sans mot de passe, ouverte par le jeu lui-même).
//!
//! Optimisation : ce thread n'existe QUE pendant une partie. Il est créé quand la
//! phase passe à "InProgress" et s'arrête dès qu'elle change. Hors partie : rien.
//! On ne garde pas le gros JSON en mémoire : on en extrait un petit résumé.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::evenement::Evenement;

/// Toutes les combien de secondes on relit la partie.
const INTERVALLE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Identifiants des objets (pour les icônes Data Dragon).
    pub items: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstantanePartie {
    pub mode: Box<str>,
    pub secondes: u32,
    pub joueurs: Vec<JoueurLive>,
}

impl InstantanePartie {
    /// Extrait le résumé depuis la réponse `/liveclientdata/allgamedata`.
    pub fn depuis_json(v: &Value) -> Option<Self> {
        let texte = |x: &Value| Box::<str>::from(x.as_str().unwrap_or("?"));
        let nombre = |x: &Value| x.as_u64().unwrap_or(0) as u16;
        let joueurs = v["allPlayers"]
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
                    .map(|l| l.iter().map(|i| nombre(&i["itemID"])).collect())
                    .unwrap_or_default(),
            })
            .collect();
        Some(Self {
            mode: texte(&v["gameData"]["gameMode"]),
            secondes: v["gameData"]["gameTime"].as_f64().unwrap_or(0.0) as u32,
            joueurs,
        })
    }
}

/// Poignée sur le thread de partie : quand on la lâche (`drop`), le thread s'arrête.
pub struct SuiviPartie {
    actif: Arc<AtomicBool>,
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
            let mut precedent: Option<InstantanePartie> = None;
            while drapeau.load(Ordering::Relaxed) {
                if let Some(p) = lire_partie() {
                    // On n'envoie que si quelque chose a changé (KDA, items, niveaux...).
                    // Le temps de jeu change toujours : on le compare à part.
                    let change = precedent.as_ref().is_none_or(|a| a.joueurs != p.joueurs);
                    if change && tx.send(Evenement::Partie(p.clone())).is_err() {
                        return;
                    }
                    precedent = Some(p);
                }
                thread::sleep(INTERVALLE);
            }
        })
        .expect("impossible de lancer le thread de partie");
    SuiviPartie { actif }
}

fn lire_partie() -> Option<InstantanePartie> {
    let v: Value = crate::http::local()
        .get("https://127.0.0.1:2999/liveclientdata/allgamedata")
        .call()
        .ok()?
        .into_json()
        .ok()?;
    InstantanePartie::depuis_json(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_une_partie() {
        let v: Value = serde_json::from_str(
            r#"{"gameData":{"gameMode":"CLASSIC","gameTime":754.3},
                "allPlayers":[{"riotIdGameName":"Faker","championName":"Ahri","team":"ORDER","level":9,
                   "scores":{"kills":3,"deaths":1,"assists":4,"creepScore":102},
                   "items":[{"itemID":3020},{"itemID":6655}]}]}"#,
        )
        .unwrap();
        let p = InstantanePartie::depuis_json(&v).unwrap();
        assert_eq!(p.secondes, 754);
        assert_eq!(&*p.mode, "CLASSIC");
        let j = &p.joueurs[0];
        assert!(j.bleu);
        assert_eq!((j.kills, j.morts, j.assists, j.cs, j.niveau), (3, 1, 4, 102, 9));
        assert_eq!(j.items, vec![3020, 6655]);
    }
}
