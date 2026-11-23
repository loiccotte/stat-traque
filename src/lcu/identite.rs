//! Conversion « puuid du client » -> « puuid de l'API Riot ».
//!
//! Observé en vrai : le client LoL donne un puuid de 36 caractères (format UUID),
//! alors que l'API Riot attend celui de 78 caractères ; avec le court, Riot répond
//! « 400 Bad Request - Exception decrypting ». On passe donc par le Riot ID :
//!   1. client : GET /lol-summoner/v2/summoners/puuid/{puuid_client} -> gameName + tagLine
//!   2. Riot   : account-v1 by-riot-id -> puuid (réponse gardée 24 h dans le cache disque)
//!
//! Coût : 1 requête locale + 1 requête Riot par compte (puis cache), ~0 Mo de RAM.
//! Ces appels sont BLOQUANTS (réseau) : à faire hors du thread du cerveau quand c'est
//! possible (scouting des alliés) ; pour mon propre compte, c'est fait une seule fois.

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
pub struct Identite {
    pub puuid: String,
    pub riot_id: String,
}

/// Convertit un puuid du client en puuid Riot. Renvoie aussi le Riot ID (utile pour
/// afficher un nom). `Err` = texte lisible pour le journal.
pub fn convertir(lcu: &Lcu, api: &RiotApi, puuid_client: &str) -> Result<Identite, String> {
    let infos = lcu
        .get(&format!("/lol-summoner/v2/summoners/puuid/{puuid_client}"))
        .ok_or("le client LoL ne connaît pas ce joueur")?;
    let (nom, tag) = riot_id(&infos).ok_or("Riot ID absent dans la réponse du client")?;
    let riot_id = format!("{nom}#{tag}");
    if est_puuid_riot(puuid_client) {
        return Ok(Identite { puuid: puuid_client.to_string(), riot_id });
    }
    let compte = api.compte(nom, tag).map_err(|e| e.to_string())?;
    let puuid = compte["puuid"].as_str().ok_or("puuid absent dans la réponse account-v1")?;
    Ok(Identite { puuid: puuid.to_string(), riot_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnait_le_format_riot() {
        // Valeurs fictives : seule la longueur compte.
        assert!(!est_puuid_riot("12345678-1234-1234-1234-123456789abc"));
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
}
