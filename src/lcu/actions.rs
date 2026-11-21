//! Actions dans le CLIENT LoL (jamais dans la partie) : page de runes, sorts
//! d'invocateur et item set en champ select. C'est ce que font Blitz, Porofessor, etc.
//!
//! Page de runes : on gère UNE seule page nommée "stat-traque ..." ; l'ancienne est
//! supprimée avant d'en créer une nouvelle, donc jamais de doublons.

use serde_json::{json, Value};

use super::Lcu;
use crate::stats::PageRunes;

const PREFIXE_PAGE: &str = "stat-traque";
const FLASH: u16 = 4;

impl Lcu {
    /// Requête avec corps JSON (POST, PUT, PATCH, DELETE). Erreur = texte lisible.
    pub fn envoyer(&self, methode: &str, chemin: &str, corps: Option<&Value>) -> Result<(), String> {
        let req = crate::http::local()
            .request(methode, &format!("https://127.0.0.1:{}{chemin}", self.port))
            .set("Authorization", &self.auth);
        let rep = match corps {
            Some(c) => req.send_json(c),
            None => req.call(),
        };
        match rep {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(code, r)) => Err(format!("{code} {}", r.into_string().unwrap_or_default())),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Remplace la page "stat-traque" par la page conseillée et la sélectionne.
pub fn appliquer_runes(lcu: &Lcu, nom: &str, page: &PageRunes) -> Result<(), String> {
    for p in lcu.get("/lol-perks/v1/pages").as_ref().and_then(Value::as_array).into_iter().flatten() {
        let a_nous = p["name"].as_str().is_some_and(|n| n.starts_with(PREFIXE_PAGE));
        if a_nous && p["isDeletable"] != false {
            if let Some(id) = p["id"].as_u64() {
                let _ = lcu.envoyer("DELETE", &format!("/lol-perks/v1/pages/{id}"), None);
            }
        }
    }
    let corps = json!({
        "name": format!("{PREFIXE_PAGE} {nom}"),
        "primaryStyleId": page.principal,
        "subStyleId": page.secondaire,
        "selectedPerkIds": page.perks,
        "current": true,
    });
    lcu.envoyer("POST", "/lol-perks/v1/pages", Some(&corps)).map_err(|e| {
        if e.contains("Max pages") || e.starts_with("400") {
            format!("plus de place pour une page de runes : supprime une de tes pages ({e})")
        } else {
            e
        }
    })
}

/// Choisit les sorts. `flash_sur_f` : Flash sur la touche F (sinon D).
pub fn appliquer_sorts(lcu: &Lcu, sorts: (u16, u16), flash_sur_f: bool) -> Result<(), String> {
    let (mut d, mut f) = sorts;
    // On met Flash du côté habituel du joueur.
    if (d == FLASH && flash_sur_f) || (f == FLASH && !flash_sur_f) {
        std::mem::swap(&mut d, &mut f);
    }
    let corps = json!({"spell1Id": d, "spell2Id": f});
    lcu.envoyer("PATCH", "/lol-champ-select/v1/session/my-selection", Some(&corps))
}

// ---------- Item sets ----------
// Le client garde TOUS les item sets d'un compte dans un seul objet JSON :
//   GET /lol-item-sets/v1/item-sets/{summonerId}/sets
//     -> {"accountId": 123, "itemSets": [ {...}, {...} ], "timestamp": 1700000000000}
//   PUT le même objet modifié pour enregistrer.
// On ne touche qu'à NOTRE set du champion (« stat-traque <champion> ») : les sets
// du joueur et ceux des autres champions restent tels quels.

/// Titre de notre item set pour un champion.
pub fn titre_item_set(nom_champion: &str) -> String {
    format!("{PREFIXE_PAGE} {nom_champion}")
}

/// Identifiant stable de notre set pour un champion (le client veut un `uid` unique
/// par set ; le même à chaque fois = on remplace au lieu d'empiler).
fn uid_item_set(champion: u16) -> String {
    format!("57a7-7a9e-{champion:05}")
}

/// Un item set au format du client. `blocs` = (titre du bloc, ids des objets).
/// Fonction pure : testée sans client.
pub fn json_item_set(champion: u16, nom_champion: &str, blocs: &[(String, Vec<u32>)]) -> Value {
    let blocs: Vec<Value> = blocs
        .iter()
        .map(|(titre, objets)| {
            json!({
                "type": titre,
                // Le client attend les ids en TEXTE.
                "items": objets.iter().map(|id| json!({"id": id.to_string(), "count": 1})).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "uid": uid_item_set(champion),
        "title": titre_item_set(nom_champion),
        "type": "custom",
        "map": "SR",
        "mode": "any",
        "priority": false,
        "sortrank": 0,
        "startedFrom": "blank",
        "associatedChampions": [champion],
        "associatedMaps": [11],
        "preferredItemSlots": [],
        "blocks": blocs,
    })
}

/// Remplace (ou ajoute) notre set dans l'objet renvoyé par le client.
/// Un ancien set est « à nous » s'il a le même uid OU le même titre.
/// Fonction pure : testée sans client.
pub fn fusionner_item_sets(existant: &Value, summoner_id: u64, set: Value, maintenant_ms: u64) -> Value {
    let mut tout = if existant.is_object() { existant.clone() } else { json!({}) };
    let (uid, titre) = (set["uid"].clone(), set["title"].clone());
    let mut sets: Vec<Value> = tout["itemSets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["uid"] != uid && s["title"] != titre)
        .cloned()
        .collect();
    sets.push(set);
    tout["itemSets"] = Value::Array(sets);
    if !tout["accountId"].is_u64() {
        tout["accountId"] = json!(summoner_id);
    }
    tout["timestamp"] = json!(maintenant_ms);
    tout
}

/// La réponse du GET est-elle sûre à réécrire ? Objet dont `itemSets` est une
/// liste ou null. Sinon on risquerait d'effacer des sets qu'on n'a pas compris.
fn reponse_item_sets_valide(v: &Value) -> bool {
    matches!(v.get("itemSets"), Some(Value::Array(_)) | Some(Value::Null))
}

/// Applique l'item set dans le client (visible dans la boutique en partie).
pub fn appliquer_item_set(lcu: &Lcu, summoner_id: u64, set: Value) -> Result<(), String> {
    let chemin = format!("/lol-item-sets/v1/item-sets/{summoner_id}/sets");
    // Sécurité : si la lecture rate, on n'écrit RIEN. Un PUT avec seulement notre
    // set effacerait tous les item sets du joueur.
    // Même chose si la réponse n'a pas la forme attendue (`itemSets` = liste, ou null
    // pour un compte sans aucun set).
    let existant = lcu
        .get(&chemin)
        .filter(reponse_item_sets_valide)
        .ok_or_else(|| "item sets du client illisibles, rien modifié".to_string())?;
    let maintenant = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let corps = fusionner_item_sets(&existant, summoner_id, set, maintenant);
    lcu.envoyer("PUT", &chemin, Some(&corps))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocs() -> Vec<(String, Vec<u32>)> {
        vec![("Core (40 parties, middle)".into(), vec![3089, 3157, 4645]), ("Bottes".into(), vec![3020])]
    }

    #[test]
    fn json_d_un_item_set() {
        let set = json_item_set(238, "Zed", &blocs());
        assert_eq!(set["title"], "stat-traque Zed");
        assert_eq!(set["associatedChampions"], json!([238]));
        assert_eq!(set["associatedMaps"], json!([11]));
        assert_eq!(set["blocks"][0]["type"], "Core (40 parties, middle)");
        let attendu = json!([{"id": "3089", "count": 1}, {"id": "3157", "count": 1}, {"id": "4645", "count": 1}]);
        assert_eq!(set["blocks"][0]["items"], attendu);
        assert_eq!(set["blocks"][1]["items"][0]["id"], "3020");
        assert_eq!(set["uid"], json_item_set(238, "Zed", &[])["uid"], "uid stable");
        assert_ne!(set["uid"], json_item_set(103, "Ahri", &[])["uid"]);
    }

    #[test]
    fn remplace_seulement_notre_set_du_champion() {
        let existant = json!({"accountId": 42, "timestamp": 1, "itemSets": [
            {"uid": "a-moi", "title": "Mon build Zed", "associatedChampions": [238]},
            {"uid": "57a7-7a9e-00238", "title": "stat-traque Zed", "blocks": []},
            {"uid": "vieux", "title": "stat-traque Zed", "blocks": []},
            {"uid": "57a7-7a9e-00103", "title": "stat-traque Ahri", "blocks": []}
        ]});
        let set = json_item_set(238, "Zed", &blocs());
        let corps = fusionner_item_sets(&existant, 42, set.clone(), 99);
        let titres: Vec<&str> =
            corps["itemSets"].as_array().unwrap().iter().map(|s| s["title"].as_str().unwrap()).collect();
        // Le set du joueur et celui d'Ahri restent ; les deux anciens « stat-traque Zed » sont remplacés.
        assert_eq!(titres, vec!["Mon build Zed", "stat-traque Ahri", "stat-traque Zed"]);
        assert_eq!(corps["itemSets"][2], set);
        assert_eq!((corps["accountId"].as_u64(), corps["timestamp"].as_u64()), (Some(42), Some(99)));
    }

    #[test]
    fn premier_item_set_du_compte() {
        // Compte sans aucun set : le client renvoie un objet sans `itemSets` (ou `null`).
        let corps = fusionner_item_sets(&json!({"itemSets": null}), 7, json_item_set(1, "Annie", &[]), 5);
        assert_eq!(corps["accountId"], 7);
        assert_eq!(corps["itemSets"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn ne_reecrit_que_une_reponse_comprise() {
        assert!(reponse_item_sets_valide(&json!({"accountId": 1, "itemSets": []})));
        assert!(reponse_item_sets_valide(&json!({"itemSets": null})));
        assert!(!reponse_item_sets_valide(&json!({"accountId": 1})));
        assert!(!reponse_item_sets_valide(&json!({"itemSets": {"0": {}}})));
        assert!(!reponse_item_sets_valide(&json!({"errorCode": "RPC_ERROR", "httpStatus": 404})));
        assert!(!reponse_item_sets_valide(&json!([])));
    }
}
