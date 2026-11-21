//! Actions dans le CLIENT LoL (jamais dans la partie) : page de runes et sorts
//! d'invocateur en champ select. C'est ce que font Blitz, Porofessor, etc.
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
