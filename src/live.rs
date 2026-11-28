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

use crate::affichage::VueLiveMoi;
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

/// Texte affiché pour un joueur en mode streamer (Riot ID vide) : on n'essaie JAMAIS
/// de retrouver son identité (règle Riot), on l'indique simplement.
pub const PSEUDO_MASQUE: &str = "(pseudo masqué)";

/// Pseudo d'un joueur de l'API live : `riotIdGameName`, sinon `riotId` (« Nom#TAG »,
/// le # peut arriver encodé %23), sinon masqué.
fn pseudo_live(j: &Value) -> Box<str> {
    let nom = j["riotIdGameName"].as_str().filter(|s| !s.trim().is_empty());
    let complet = || {
        let id = j["riotId"].as_str()?.replace("%23", "#");
        let nom = id.split('#').next()?.trim().to_string();
        (!nom.is_empty()).then_some(nom)
    };
    match nom {
        Some(n) => n.into(),
        None => complet().map_or(PSEUDO_MASQUE.into(), Box::from),
    }
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
                pseudo: pseudo_live(j),
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

/// Réduit un nom de champion à ses lettres et chiffres minuscules (« Kai'Sa » -> « kaisa »),
/// pour comparer le nom du jeu et celui de Data Dragon sans se soucier de la ponctuation.
fn normaliser(nom: &str) -> String {
    nom.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Or actuel dans la réponse de `/liveclientdata/activeplayer` (champ `currentGold`,
/// uniquement MON joueur). `None` si le champ manque.
///
/// Pas encore branché : `activeplayer` n'est pas demandé aujourd'hui (règle de la tâche :
/// aucune requête de plus). Prêt pour le jour où cette route sera lue.
#[allow(dead_code)]
pub fn or_actuel(activeplayer: &Value) -> Option<u32> {
    let or = activeplayer["currentGold"].as_f64()?;
    (or >= 0.0).then_some(or as u32)
}

impl InstantanePartie {
    /// MON joueur dans la liste, retrouvé par mon champion (connu depuis le champ select).
    /// `playerlist` ne dit pas qui est le joueur local : on ne trouve donc « moi » que si
    /// le nom du champion est unique dans la partie, sinon `None` (jamais de devinette).
    pub fn mon_joueur(&self, mon_champion: &str) -> Option<&JoueurLive> {
        let cible = normaliser(mon_champion);
        if cible.is_empty() {
            return None;
        }
        let mut trouves = self.joueurs.iter().filter(|j| normaliser(&j.champion) == cible);
        let premier = trouves.next()?;
        trouves.next().is_none().then_some(premier)
    }

    /// Mes chiffres pour la petite fenêtre : temps, niveau, CS (déjà dans `playerlist` et
    /// `gamestats`) et or (`or`, venant de `activeplayer` si on le lit un jour ; 0 sinon).
    /// `None` si je ne suis pas retrouvé ou si le niveau est illisible.
    /// Aucune donnée d'un adversaire n'est lue ici.
    pub fn vue_moi(&self, mon_champion: &str, or: Option<u32>) -> Option<VueLiveMoi> {
        let moi = self.mon_joueur(mon_champion).filter(|j| j.niveau > 0)?;
        Some(VueLiveMoi { secondes: self.secondes, cs: moi.cs, niveau: moi.niveau, or: or.unwrap_or(0), forme: None })
    }
}

// L'« or contre l'adversaire » (estimé à partir des objets adverses) n'est volontairement
// PAS implémenté : il sera validé (ou non) par l'audit de conformité Riot T24.

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

    fn partie_simulee(joueurs: &str, stats: &str) -> InstantanePartie {
        let j: Value = serde_json::from_str(joueurs).unwrap();
        let s: Value = serde_json::from_str(stats).unwrap();
        InstantanePartie::depuis_json(&j, &s).unwrap()
    }

    const DEUX_JOUEURS: &str = r#"[
        {"riotIdGameName":"Alice","championName":"Kai'Sa","team":"ORDER","level":11,"scores":{"creepScore":87}},
        {"riotIdGameName":"Bob","championName":"Ahri","team":"CHAOS","level":12,"scores":{"creepScore":95}}]"#;

    #[test]
    fn mes_chiffres_partie_normale() {
        let p = partie_simulee(DEUX_JOUEURS, r#"{"gameMode":"CLASSIC","gameTime":812.9}"#);
        // La ponctuation du nom ne gêne pas ; on lit MON entrée, pas celle de l'adversaire.
        let v = p.vue_moi("Kaisa", Some(1350)).unwrap();
        assert_eq!(v, VueLiveMoi { secondes: 812, cs: 87, niveau: 11, or: 1350, forme: None });
        // Sans activeplayer : or à 0.
        assert_eq!(p.vue_moi("Kai'Sa", None).unwrap().or, 0);
    }

    #[test]
    fn mes_chiffres_mode_streamer() {
        // Riot ID vide : sans importance, on me retrouve par mon champion.
        let p = partie_simulee(
            r#"[{"riotIdGameName":"","riotId":"","championName":"Ahri","team":"ORDER","level":5,"scores":{"creepScore":30}}]"#,
            r#"{"gameMode":"CLASSIC","gameTime":400.0}"#,
        );
        let v = p.vue_moi("Ahri", None).unwrap();
        assert_eq!((v.cs, v.niveau, v.secondes), (30, 5, 400));
    }

    #[test]
    fn mes_chiffres_champ_manquant_ou_inconnu() {
        // Niveau absent -> illisible.
        let p = partie_simulee(
            r#"[{"championName":"Ahri","team":"ORDER","scores":{"creepScore":30}}]"#,
            r#"{"gameMode":"CLASSIC","gameTime":400.0}"#,
        );
        assert_eq!(p.vue_moi("Ahri", None), None);
        // Mon champion absent de la liste, ou vide.
        let p = partie_simulee(DEUX_JOUEURS, r#"{"gameTime":10.0}"#);
        assert_eq!(p.vue_moi("Zed", None), None);
        assert_eq!(p.vue_moi("", None), None);
        // Même champion deux fois : on ne devine pas.
        let double = r#"[{"championName":"Ahri","level":3},{"championName":"Ahri","level":4}]"#;
        assert_eq!(partie_simulee(double, "{}").vue_moi("Ahri", None), None);
    }

    #[test]
    fn hors_partie_rien_a_lire() {
        // Le jeu ne répond pas / pas une liste : pas de résumé du tout.
        let rien = Value::Null;
        assert!(InstantanePartie::depuis_json(&rien, &rien).is_none());
    }

    #[test]
    fn or_depuis_activeplayer() {
        let v = |t: &str| serde_json::from_str::<Value>(t).unwrap();
        assert_eq!(or_actuel(&v(r#"{"currentGold":1234.56}"#)), Some(1234));
        assert_eq!(or_actuel(&v(r#"{}"#)), None);
    }

    #[test]
    fn pseudo_masque_en_mode_streamer() {
        let v = |t: &str| serde_json::from_str::<Value>(t).unwrap();
        assert_eq!(&*pseudo_live(&v(r#"{"riotIdGameName":"","riotId":""}"#)), PSEUDO_MASQUE);
        assert_eq!(&*pseudo_live(&v(r#"{}"#)), PSEUDO_MASQUE);
        assert_eq!(&*pseudo_live(&v(r#"{"riotIdGameName":"","riotId":"Nom%23EUW"}"#)), "Nom");
        assert_eq!(&*pseudo_live(&v(r#"{"riotIdGameName":"Nom"}"#)), "Nom");
    }
}
