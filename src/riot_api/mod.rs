//! Client de l'API Riot (internet, demande la clé API).
//!
//! Chaque requête passe par :
//!   1. le CACHE disque (si la réponse est déjà connue : aucune requête réseau) ;
//!   2. le LIMITEUR (on ne dépasse jamais les limites de la clé) ;
//!   3. le réseau.
//!
//! Le client est partageable entre threads (`Arc<RiotApi>`) : pour les 10 joueurs
//! d'une partie, plusieurs threads peuvent l'utiliser en même temps.

// Les fonctions compte / rangs / parties serviront à la partie joueur.
#![allow(dead_code)]

pub mod cache;
pub mod limiteur;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

use cache::{Cache, Duree};
use limiteur::Limiteur;

#[derive(Debug, PartialEq, Eq)]
pub enum ErreurApi {
    /// Aucune clé enregistrée.
    PasDeCle,
    /// 401 / 403 : clé expirée (les clés de dev durent 24 h) ou invalide.
    CleInvalide,
    /// 404 : joueur / partie introuvable.
    Introuvable,
    /// Pas d'internet, ou Riot ne répond pas.
    Reseau,
    Autre(u16),
}

impl std::fmt::Display for ErreurApi {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ErreurApi::PasDeCle => write!(f, "aucune clé API. Ajoute-la avec :  stat-traque cle RGAPI-xxxxxxxx"),
            ErreurApi::CleInvalide => write!(
                f,
                "clé API EXPIRÉE ou invalide. Régénère-la sur https://developer.riotgames.com \
                 puis lance :  stat-traque cle RGAPI-xxxxxxxx"
            ),
            ErreurApi::Introuvable => write!(f, "introuvable"),
            ErreurApi::Reseau => write!(f, "impossible de joindre Riot (internet ?)"),
            ErreurApi::Autre(code) => write!(f, "réponse inattendue de Riot (code {code})"),
        }
    }
}

/// Serveur "régional" à utiliser pour account-v1 et match-v5, selon la plateforme.
pub fn region_routage(plateforme: &str) -> &'static str {
    match plateforme {
        "na1" | "br1" | "la1" | "la2" => "americas",
        "kr" | "jp1" => "asia",
        "oc1" | "ph2" | "sg2" | "th2" | "tw2" | "vn2" => "sea",
        _ => "europe", // euw1, eun1, tr1, ru, me1
    }
}

// ---------- Compteurs de requêtes ----------
// Pour MESURER combien de requêtes coûte chaque écran (partie, synchro de l'historique...).
// Un compteur atomique se modifie depuis plusieurs threads sans verrou.

/// Requêtes réellement envoyées à Riot (celles qui comptent pour les limites de la clé).
static REQUETES_RESEAU: AtomicU64 = AtomicU64::new(0);
/// Réponses servies par le cache disque (0 requête).
static REPONSES_CACHE: AtomicU64 = AtomicU64::new(0);

/// Instantané des compteurs : on en prend un avant une opération, puis
/// `Compteurs::depuis(avant)` donne ce que l'opération a coûté.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Compteurs {
    pub reseau: u64,
    pub cache: u64,
}

impl Compteurs {
    pub fn maintenant() -> Self {
        Self { reseau: REQUETES_RESEAU.load(Ordering::Relaxed), cache: REPONSES_CACHE.load(Ordering::Relaxed) }
    }

    /// Ce qui a été consommé depuis `avant`. Approximatif si plusieurs opérations
    /// tournent en même temps (les compteurs sont communs à toute l'app).
    pub fn depuis(avant: Self) -> Self {
        let n = Self::maintenant();
        Self { reseau: n.reseau.saturating_sub(avant.reseau), cache: n.cache.saturating_sub(avant.cache) }
    }
}

impl std::fmt::Display for Compteurs {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} requêtes Riot, {} réponses du cache", self.reseau, self.cache)
    }
}

pub struct RiotApi {
    cle: Option<String>,
    plateforme: String,
    limiteur: &'static Limiteur,
    cache: Cache,
}

impl RiotApi {
    /// Crée le client avec la clé et la région enregistrées.
    pub fn depuis_reglages(reglages: &crate::stockage::Reglages) -> Self {
        Self {
            cle: crate::stockage::lire_cle(),
            plateforme: reglages.region().to_string(),
            limiteur: Limiteur::global(),
            cache: Cache::nouveau(crate::stockage::dossier_cache("riot")),
        }
    }

    /// Requête générique. `hote` = "euw1" ou "europe"...
    fn requete(&self, hote: &str, chemin: &str, duree: Duree) -> Result<Value, ErreurApi> {
        self.requete_options(hote, chemin, duree, true)
    }

    /// Comme `requete`, mais `lire_cache = false` force une réponse fraîche de Riot
    /// (la réponse est quand même ÉCRITE dans le cache pour les suivants).
    /// Sert après une partie : mes LP viennent de changer, le cache de 10 min est périmé.
    fn requete_options(&self, hote: &str, chemin: &str, duree: Duree, lire_cache: bool) -> Result<Value, ErreurApi> {
        let url = format!("https://{hote}.api.riotgames.com{chemin}");
        if let Some(texte) = self.cache.lire(&url).filter(|_| lire_cache) {
            match serde_json::from_str(&texte) {
                Ok(v) => {
                    REPONSES_CACHE.fetch_add(1, Ordering::Relaxed);
                    return Ok(v);
                }
                Err(_) => self.cache.oublier(&url), // fichier abîmé : on redemande à Riot
            }
        }
        let cle = self.cle.as_deref().ok_or(ErreurApi::PasDeCle)?;
        // 2 essais maximum : le 2e seulement si Riot a répondu 429 (trop de requêtes).
        for _ in 0..2 {
            self.limiteur.attendre();
            REQUETES_RESEAU.fetch_add(1, Ordering::Relaxed);
            match crate::http::internet().get(&url).set("X-Riot-Token", cle).call() {
                Ok(rep) => {
                    let texte = rep.into_string().map_err(|_| ErreurApi::Reseau)?;
                    let valeur = serde_json::from_str(&texte).map_err(|_| ErreurApi::Autre(0))?;
                    self.cache.ecrire(&url, &texte, duree);
                    return Ok(valeur);
                }
                Err(ureq::Error::Status(429, rep)) => {
                    let secondes = rep.header("Retry-After").and_then(|s| s.parse().ok()).unwrap_or(5);
                    self.limiteur.pause(Duration::from_secs(secondes));
                }
                Err(ureq::Error::Status(401 | 403, _)) => return Err(ErreurApi::CleInvalide),
                Err(ureq::Error::Status(404, _)) => return Err(ErreurApi::Introuvable),
                Err(ureq::Error::Status(code, _)) => return Err(ErreurApi::Autre(code)),
                Err(ureq::Error::Transport(_)) => return Err(ErreurApi::Reseau),
            }
        }
        Err(ErreurApi::Autre(429))
    }

    /// Ménage du cache disque (à lancer au démarrage, en arrière-plan).
    pub fn nettoyer_cache(&self) -> usize {
        self.cache.nettoyer()
    }

    /// Vérifie que la clé fonctionne (statut du serveur, jamais mis en cache).
    pub fn verifier_cle(&self) -> Result<(), ErreurApi> {
        self.requete(&self.plateforme, "/lol/status/v4/platform-data", Duree::Pendant(Duration::ZERO))
            .map(|_| ())
    }

    /// Riot ID ("Pseudo#TAG") -> compte (contient le `puuid`). Change très rarement.
    pub fn compte(&self, nom: &str, tag: &str) -> Result<Value, ErreurApi> {
        let chemin = format!("/riot/account/v1/accounts/by-riot-id/{}/{}", encoder(nom), encoder(tag));
        let hote = match region_routage(&self.plateforme) {
            "sea" => "asia", // account-v1 n'existe pas sur "sea"
            r => r,
        };
        self.requete(hote, &chemin, Duree::Pendant(Duration::from_secs(24 * 3600)))
    }

    /// Rangs (solo / flex) d'un joueur. Change après chaque partie : cache court.
    pub fn rangs(&self, puuid: &str) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/league/v4/entries/by-puuid/{puuid}");
        self.requete(&self.plateforme, &chemin, Duree::Pendant(Duration::from_secs(600)))
    }

    /// Rangs d'un joueur en ignorant le cache (mes LP juste après une partie).
    pub fn rangs_frais(&self, puuid: &str) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/league/v4/entries/by-puuid/{puuid}");
        self.requete_options(&self.plateforme, &chemin, Duree::Pendant(Duration::from_secs(600)), false)
    }

    /// Partie en cours d'un joueur (spectator-v5) : les 10 participants avec leur puuid.
    /// 404 (`Introuvable`) s'il n'est pas en partie.
    pub fn partie_en_cours(&self, puuid: &str) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/spectator/v5/active-games/by-summoner/{puuid}");
        self.requete(&self.plateforme, &chemin, Duree::Pendant(Duration::from_secs(30)))
    }

    /// Maîtrise d'un joueur sur un champion (niveau, points) : bon indice « all time ».
    pub fn maitrise(&self, puuid: &str, champion: u16) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/champion-mastery/v4/champion-masteries/by-puuid/{puuid}/by-champion/{champion}");
        self.requete(&self.plateforme, &chemin, Duree::Pendant(Duration::from_secs(6 * 3600)))
    }

    /// Joueurs Challenger / Grandmaster / Master en solo/duo (pour la collecte de stats).
    /// `ligue` = "challengerleagues", "grandmasterleagues" ou "masterleagues".
    pub fn ligue_haut_niveau(&self, ligue: &str) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/league/v4/{ligue}/by-queue/RANKED_SOLO_5x5");
        self.requete(&self.plateforme, &chemin, Duree::Pendant(Duration::from_secs(3600)))
    }

    /// Une page (205 joueurs au plus) d'un palier / d'une division en solo/duo, pour
    /// échantillonner les paliers sous Master (références de la fiche joueur).
    /// `palier` = "GOLD"..., `division` = "I" à "IV", `page` à partir de 1. Cache 1 h.
    pub fn entrees_ligue(&self, palier: &str, division: &str, page: u32) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/league/v4/entries/RANKED_SOLO_5x5/{palier}/{division}?page={}", page.max(1));
        self.requete(&self.plateforme, &chemin, Duree::Pendant(Duration::from_secs(3600)))
    }

    /// Dernières parties CLASSÉES solo/duo (file 420) d'un joueur.
    pub fn ids_parties_classees(&self, puuid: &str, nombre: u8) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/match/v5/matches/by-puuid/{puuid}/ids?queue=420&type=ranked&count={nombre}");
        self.requete(region_routage(&self.plateforme), &chemin, Duree::Pendant(Duration::from_secs(600)))
    }

    /// Détail d'une partie SANS la garder sur le disque (collecte de masse : on n'en
    /// garde que les stats agrégées, pas les dizaines de Ko de JSON par partie).
    pub fn partie_sans_cache(&self, id: &str) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/match/v5/matches/{id}");
        self.requete(region_routage(&self.plateforme), &chemin, Duree::Pendant(Duration::ZERO))
    }

    pub fn plateforme(&self) -> &str {
        &self.plateforme
    }

    /// Identifiants des dernières parties d'un joueur.
    pub fn ids_parties(&self, puuid: &str, nombre: u8) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/match/v5/matches/by-puuid/{puuid}/ids?count={nombre}");
        self.requete(region_routage(&self.plateforme), &chemin, Duree::Pendant(Duration::from_secs(120)))
    }

    /// Identifiants des parties (TOUTES files) terminées après `depuis` (secondes Unix,
    /// 0 = pas de limite), les plus récentes d'abord. `nombre` : 100 au maximum (limite Riot).
    /// Cache court (30 s) : la liste change à chaque partie jouée.
    pub fn ids_parties_depuis(&self, puuid: &str, depuis: u64, nombre: u8) -> Result<Value, ErreurApi> {
        let debut = if depuis > 0 { format!("&startTime={depuis}") } else { String::new() };
        let chemin = format!("/lol/match/v5/matches/by-puuid/{puuid}/ids?count={}{debut}", nombre.min(100));
        self.requete(region_routage(&self.plateforme), &chemin, Duree::Pendant(Duration::from_secs(30)))
    }

    /// Détail d'une partie terminée : ne change jamais -> cache permanent.
    pub fn partie(&self, id: &str) -> Result<Value, ErreurApi> {
        let chemin = format!("/lol/match/v5/matches/{id}");
        self.requete(region_routage(&self.plateforme), &chemin, Duree::Toujours)
    }
}

/// Encode un texte pour l'URL (espaces, accents, caractères spéciaux des pseudos).
fn encoder(texte: &str) -> String {
    texte
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routage_par_region() {
        assert_eq!(region_routage("euw1"), "europe");
        assert_eq!(region_routage("na1"), "americas");
        assert_eq!(region_routage("kr"), "asia");
        assert_eq!(region_routage("vn2"), "sea");
    }

    #[test]
    fn encode_les_pseudos() {
        assert_eq!(encoder("Zed King"), "Zed%20King");
        assert_eq!(encoder("Élu"), "%C3%89lu");
    }
}
