//! Enregistreur : pendant que tu joues normalement, l'app note tout ce que le
//! client LoL et le jeu lui envoient, dans `%APPDATA%\stat-traque\enregistrements\`.
//! Ces fichiers se REJOUENT ensuite (`stat-traque --rejouer fichier`) : on peut
//! tester et corriger l'app sur de vraies parties, sans relancer LoL.
//!
//! - Pseudos, Riot ID et identifiants sont ANONYMISÉS avant l'écriture
//!   (remplacés par un alias stable, ex. "J-3fa2" : le même joueur garde le même alias).
//! - Taille bornée : 5 sessions gardées, 30 Mo maximum par session.
//! - Désactivable : `enregistrer=non` dans reglages.txt.
//!
//! Format : une ligne JSON par événement (« JSON Lines ») :
//!   {"t": ms depuis le début, "src": "ws" | "get" | "live", ...}

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};

const SESSIONS_GARDEES: usize = 5;
const TAILLE_MAX: u64 = 30 * 1024 * 1024;

struct Session {
    fichier: BufWriter<File>,
    debut: Instant,
    ecrit: u64,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn dossier() -> PathBuf {
    let d = crate::stockage::dossier_app().join("enregistrements");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Ouvre un nouveau fichier de session (sauf si désactivé dans les réglages).
pub fn demarrer(reglages: &crate::stockage::Reglages) {
    if reglages.get("enregistrer") == Some("non") {
        return;
    }
    let dossier = dossier();
    nettoyer(&dossier);
    let secondes = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if let Ok(f) = File::create(dossier.join(format!("session-{secondes}.jsonl"))) {
        *SESSION.lock().unwrap() = Some(Session { fichier: BufWriter::new(f), debut: Instant::now(), ecrit: 0 });
    }
}

/// Garde seulement les sessions les plus récentes (le nom contient la date).
fn nettoyer(dossier: &std::path::Path) {
    let mut fichiers: Vec<PathBuf> = std::fs::read_dir(dossier)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    fichiers.sort();
    let en_trop = (fichiers.len() + 1).saturating_sub(SESSIONS_GARDEES);
    for f in fichiers.into_iter().take(en_trop) {
        let _ = std::fs::remove_file(f);
    }
}

/// L'enregistrement est-il actif ? (pour éviter de préparer des données pour rien)
pub fn actif() -> bool {
    SESSION.lock().unwrap().is_some()
}

/// Écrit une ligne. `ligne` doit être un objet JSON ; on y ajoute "t".
pub fn noter(mut ligne: Value) {
    let mut garde = SESSION.lock().unwrap();
    let Some(s) = garde.as_mut() else { return };
    if s.ecrit > TAILLE_MAX {
        return;
    }
    anonymiser(&mut ligne);
    ligne["t"] = json!(s.debut.elapsed().as_millis() as u64);
    let texte = ligne.to_string();
    s.ecrit += texte.len() as u64 + 1;
    let _ = writeln!(s.fichier, "{texte}");
    // On vide le tampon à chaque ligne : rien n'est perdu si l'app est tuée.
    let _ = s.fichier.flush();
}

/// Clés dont la valeur identifie un joueur réel.
const CLES_PRIVEES: [&str; 14] = [
    "gameName", "riotIdGameName", "riotIdTagLine", "tagLine", "riotId", "summonerName",
    "displayName", "puuid", "summonerId", "accountId", "playerId", "obfuscatedSummonerName",
    "obfuscatedPuuid", "internalName",
];

fn alias(texte: &str) -> String {
    let h = texte.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("J-{:04x}", h & 0xffff)
}

/// Remplace (récursivement) les valeurs privées par un alias stable.
pub fn anonymiser(v: &mut Value) {
    match v {
        Value::Object(o) => {
            for (cle, val) in o.iter_mut() {
                if CLES_PRIVEES.contains(&cle.as_str()) {
                    match val {
                        Value::String(s) if !s.is_empty() => *s = alias(s),
                        Value::Number(_) => *val = json!(0),
                        _ => {}
                    }
                } else {
                    anonymiser(val);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(anonymiser),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymise_les_pseudos_mais_pas_les_champions() {
        let mut v = json!({"allPlayers": [{"riotIdGameName": "Loic", "championName": "Zed", "summonerId": 42}],
                           "myTeam": [{"puuid": "abc", "championId": 238}]});
        anonymiser(&mut v);
        let pseudo = v["allPlayers"][0]["riotIdGameName"].as_str().unwrap();
        assert!(pseudo.starts_with("J-") && pseudo != "Loic");
        assert_eq!(pseudo, alias("Loic")); // stable
        assert_eq!(v["allPlayers"][0]["championName"], "Zed");
        assert_eq!(v["allPlayers"][0]["summonerId"], 0);
        assert_eq!(v["myTeam"][0]["championId"], 238);
    }
}
