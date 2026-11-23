//! Conversion « puuid du client » -> « puuid de l'API Riot ».
//!
//! Observé en vrai : le client LoL donne un puuid de 36 caractères (format UUID),
//! alors que l'API Riot attend celui de 78 caractères ; avec le court, Riot répond
//! « 400 Bad Request - Exception decrypting ». On passe donc par le Riot ID :
//!   1. client : GET /lol-summoner/v2/summoners/puuid/{puuid_client} -> gameName + tagLine
//!   2. Riot   : account-v1 by-riot-id -> puuid (réponse gardée 24 h dans le cache disque)
//!
//! Coût : 1 requête locale + 1 requête Riot par compte (puis cache), ~0 Mo de RAM.
//! Ces appels sont BLOQUANTS (réseau) : toujours dans un thread à part, jamais dans
//! celui du cerveau.
//!
//! Pour MON compte, la correspondance est aussi gardée sur le disque
//! (`%APPDATA%\stat-traque\comptes.tsv`, une ligne `puuid_client<TAB>puuid_riot`) :
//! ma fiche et mon historique restent accessibles même quand la clé API a expiré.

use std::path::PathBuf;

use serde_json::Value;

use super::Lcu;
use crate::riot_api::RiotApi;

/// Le puuid a-t-il déjà le format de l'API Riot ? (78 caractères ; le court en fait 36)
pub fn est_puuid_riot(puuid: &str) -> bool {
    puuid.len() >= 70
}

/// Riot ID (gameName, tagLine) dans une réponse de /lol-summoner/v2/summoners/puuid/...
/// None si l'un des deux manque ou est vide.
pub fn riot_id(reponse: &Value) -> Option<(&str, &str)> {
    let nom = reponse["gameName"].as_str().filter(|s| !s.is_empty())?;
    let tag = reponse["tagLine"].as_str().filter(|s| !s.is_empty())?;
    Some((nom, tag))
}

/// Résultat d'une conversion : le puuid Riot et le Riot ID « Pseudo#TAG ».
#[derive(Debug, PartialEq, Eq)]
pub struct Identite {
    pub puuid: String,
    pub riot_id: String,
}

/// D'où viennent les réponses : le vrai client + l'API Riot, ou un faux pour les tests.
pub trait SourceIdentite {
    /// Riot ID (nom, tag) d'un joueur connu du client.
    fn riot_id_du_client(&self, puuid_client: &str) -> Option<(String, String)>;
    /// puuid Riot (78 car.) d'un Riot ID.
    fn puuid_par_riot_id(&self, nom: &str, tag: &str) -> Result<String, String>;
}

/// La vraie source : client LoL + API Riot.
pub struct Reelle<'a> {
    pub lcu: &'a Lcu,
    pub api: &'a RiotApi,
}

impl SourceIdentite for Reelle<'_> {
    fn riot_id_du_client(&self, puuid_client: &str) -> Option<(String, String)> {
        let infos = self.lcu.get(&format!("/lol-summoner/v2/summoners/puuid/{puuid_client}"))?;
        riot_id(&infos).map(|(n, t)| (n.to_string(), t.to_string()))
    }

    fn puuid_par_riot_id(&self, nom: &str, tag: &str) -> Result<String, String> {
        let compte = self.api.compte(nom, tag).map_err(|e| e.to_string())?;
        compte["puuid"].as_str().map(String::from).ok_or_else(|| "puuid absent dans la réponse account-v1".into())
    }
}

/// Convertit un puuid du client en puuid Riot, avec le Riot ID (utile pour afficher
/// un nom). `Err` = texte lisible pour le journal.
pub fn convertir(source: &impl SourceIdentite, puuid_client: &str) -> Result<Identite, String> {
    let (nom, tag) = source.riot_id_du_client(puuid_client).ok_or("le client LoL ne donne pas le Riot ID de ce joueur")?;
    let riot_id = format!("{nom}#{tag}");
    if est_puuid_riot(puuid_client) {
        return Ok(Identite { puuid: puuid_client.to_string(), riot_id });
    }
    let puuid = source.puuid_par_riot_id(&nom, &tag)?;
    Ok(Identite { puuid, riot_id })
}

// ---------- Mémoire disque de MON compte (client -> Riot) ----------

fn chemin_comptes() -> PathBuf {
    crate::stockage::dossier_app().join("comptes.tsv")
}

/// Cherche `puuid_client` dans le texte du fichier comptes.tsv. Fonction pure.
pub fn chercher(texte: &str, puuid_client: &str) -> Option<String> {
    texte.lines().filter_map(|l| l.split_once('\t')).find(|(c, _)| *c == puuid_client).map(|(_, r)| r.trim().to_string())
}

/// Texte du fichier après ajout / remplacement d'une correspondance. Fonction pure.
pub fn avec_correspondance(texte: &str, puuid_client: &str, puuid_riot: &str) -> String {
    let mut lignes: Vec<String> = texte
        .lines()
        .filter(|l| l.split_once('\t').is_some_and(|(c, _)| c != puuid_client))
        .map(String::from)
        .collect();
    lignes.push(format!("{puuid_client}\t{puuid_riot}"));
    lignes.join("\n") + "\n"
}

/// puuid Riot de mon compte déjà connu (0 requête).
pub fn memoire_lire(puuid_client: &str) -> Option<String> {
    chercher(&std::fs::read_to_string(chemin_comptes()).ok()?, puuid_client).filter(|p| est_puuid_riot(p))
}

/// Retient la correspondance de mon compte (quelques lignes au plus : un par compte).
pub fn memoire_ecrire(puuid_client: &str, puuid_riot: &str) {
    let ancien = std::fs::read_to_string(chemin_comptes()).unwrap_or_default();
    if chercher(&ancien, puuid_client).as_deref() == Some(puuid_riot) {
        return;
    }
    let _ = std::fs::write(chemin_comptes(), avec_correspondance(&ancien, puuid_client, puuid_riot));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Faux client + fausse API (valeurs fictives).
    struct Fausse {
        connu: bool,
        riot_ok: bool,
        appels_riot: Cell<u32>,
    }

    impl SourceIdentite for Fausse {
        fn riot_id_du_client(&self, _p: &str) -> Option<(String, String)> {
            self.connu.then(|| ("Joueur Test".to_string(), "EUW".to_string()))
        }
        fn puuid_par_riot_id(&self, _n: &str, _t: &str) -> Result<String, String> {
            self.appels_riot.set(self.appels_riot.get() + 1);
            if self.riot_ok { Ok("r".repeat(78)) } else { Err("clé API EXPIRÉE".into()) }
        }
    }

    const COURT: &str = "12345678-1234-1234-1234-123456789abc";

    #[test]
    fn reconnait_le_format_riot() {
        assert!(!est_puuid_riot(COURT));
        assert!(est_puuid_riot(&"a".repeat(78)));
    }

    #[test]
    fn lit_le_riot_id_du_client() {
        let v: Value = serde_json::from_str(r#"{"gameName":"Joueur Test","tagLine":"EUW"}"#).unwrap();
        assert_eq!(riot_id(&v), Some(("Joueur Test", "EUW")));
        let vide: Value = serde_json::from_str(r#"{"gameName":"","tagLine":"EUW"}"#).unwrap();
        assert_eq!(riot_id(&vide), None);
        assert_eq!(riot_id(&Value::Null), None);
    }

    #[test]
    fn convertit_ou_explique_l_echec() {
        let ok = Fausse { connu: true, riot_ok: true, appels_riot: Cell::new(0) };
        let id = convertir(&ok, COURT).unwrap();
        assert_eq!((id.puuid.len(), id.riot_id.as_str()), (78, "Joueur Test#EUW"));
        // Déjà au format Riot : pas d'appel à account-v1.
        let deja = Fausse { connu: true, riot_ok: true, appels_riot: Cell::new(0) };
        convertir(&deja, &"x".repeat(78)).unwrap();
        assert_eq!(deja.appels_riot.get(), 0);
        // Joueur inconnu du client, ou clé expirée : erreur lisible, rien d'inventé.
        assert!(convertir(&Fausse { connu: false, riot_ok: true, appels_riot: Cell::new(0) }, COURT).is_err());
        let err = convertir(&Fausse { connu: true, riot_ok: false, appels_riot: Cell::new(0) }, COURT).unwrap_err();
        assert!(err.contains("EXPIRÉE"));
    }

    #[test]
    fn memoire_des_comptes() {
        let t = avec_correspondance("", "c1", "r1");
        let t = avec_correspondance(&t, "c2", "r2");
        let t = avec_correspondance(&t, "c1", "r1bis"); // remplacement, pas de doublon
        assert_eq!(chercher(&t, "c1").as_deref(), Some("r1bis"));
        assert_eq!(chercher(&t, "c2").as_deref(), Some("r2"));
        assert_eq!(chercher(&t, "c3"), None);
        assert_eq!(t.lines().count(), 2);
    }
}
