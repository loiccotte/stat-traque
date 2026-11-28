//! Lecture de la session de champ select (`/lol-champ-select/v1/session`).
//!
//! On en tire : mon champion (survolé ou verrouillé), mon rôle, les champions de
//! mes alliés (et leur `puuid` SI le client le donne), et ceux des ennemis.
//!
//! Conformité Riot : en partie CLASSÉE, le client cache l'identité des alliés
//! (puuid vide). On ne cherche JAMAIS à la retrouver : on ne lit que ce qui est donné.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allie {
    /// 0 = pas encore choisi.
    pub champion: u16,
    pub role: Option<Box<str>>,
    /// Présent seulement hors partie classée (le client le cache sinon).
    pub puuid: Option<Box<str>>,
    pub moi: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChampSelect {
    /// Mon champion : verrouillé, sinon survolé. 0 = rien.
    pub champion: u16,
    pub role: Option<Box<str>>,
    /// true quand j'ai validé mon choix (c'est là qu'on applique les runes).
    pub verrouille: bool,
    pub allies: Vec<Allie>,
    /// Champions ennemis visibles (verrouillés). Vide en partie à l'aveugle.
    pub ennemis: Vec<u16>,
}

impl ChampSelect {
    /// Champions alliés choisis (moi compris), pour l'analyse de compo.
    pub fn champions_allies(&self) -> impl Iterator<Item = u16> + '_ {
        self.allies.iter().map(|a| a.champion).filter(|&c| c != 0)
    }
}

fn texte(v: &Value) -> Option<Box<str>> {
    v.as_str().filter(|s| !s.is_empty()).map(Box::from)
}

/// Champion d'une ligne : verrouillé (`championId`), sinon intention (`championPickIntent`).
fn champion_de(j: &Value) -> u16 {
    match j["championId"].as_u64() {
        Some(id) if id != 0 => id as u16,
        _ => j["championPickIntent"].as_u64().unwrap_or(0) as u16,
    }
}

/// Lit la session. `None` si le format est inattendu.
pub fn lire(session: &Value) -> Option<ChampSelect> {
    let ma_case = session["localPlayerCellId"].as_i64()?;
    let allies: Vec<Allie> = session["myTeam"]
        .as_array()?
        .iter()
        .map(|j| Allie {
            champion: champion_de(j),
            role: texte(&j["assignedPosition"]),
            // Second garde-fou : un joueur dont le client masque le nom n'a pas de puuid
            // utilisable, même si le champ n'est pas vide.
            puuid: if j["nameVisibilityType"] == "HIDDEN" { None } else { texte(&j["puuid"]) },
            moi: j["cellId"].as_i64() == Some(ma_case),
        })
        .collect();
    let moi = allies.iter().find(|a| a.moi)?;
    let ennemis = session["theirTeam"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|j| j["championId"].as_u64())
        .filter(|&id| id != 0)
        .map(|id| id as u16)
        .collect();
    // Mes actions de pick (un tour de draft = une liste d'actions).
    let mes_picks: Vec<&Value> = session["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|tour| tour.as_array().into_iter().flatten())
        .filter(|a| a["actorCellId"].as_i64() == Some(ma_case) && a["type"] == "pick")
        .collect();
    // Verrouillé = une action "pick" de ma case est terminée.
    let verrouille = mes_picks.iter().any(|a| a["completed"] == true);
    // Observé en vrai : pendant le simple SURVOL, le champion n'est ni dans
    // /current-champion (0) ni forcément dans myTeam ; il est dans actions[].championId.
    let champion = match moi.champion {
        0 => mes_picks.iter().filter_map(|a| a["championId"].as_u64()).find(|&id| id != 0).unwrap_or(0) as u16,
        c => c,
    };
    Some(ChampSelect { champion, role: moi.role.clone(), verrouille, ennemis, allies })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_les_deux_equipes() {
        let session: Value = serde_json::from_str(
            r#"{"localPlayerCellId":1,
                "myTeam":[{"cellId":0,"championId":54,"assignedPosition":"top","puuid":""},
                          {"cellId":1,"championId":238,"assignedPosition":"middle","puuid":"moi-puuid"},
                          {"cellId":2,"championId":0,"championPickIntent":64,"assignedPosition":"jungle","puuid":""}],
                "theirTeam":[{"cellId":5,"championId":157},{"cellId":6,"championId":0}],
                "actions":[[{"actorCellId":1,"type":"ban","completed":true}],
                           [{"actorCellId":1,"type":"pick","completed":true,"championId":238}]]}"#,
        )
        .unwrap();
        let cs = lire(&session).unwrap();
        assert_eq!((cs.champion, cs.role.as_deref(), cs.verrouille), (238, Some("middle"), true));
        assert_eq!(cs.champions_allies().collect::<Vec<_>>(), vec![54, 238, 64]);
        assert_eq!(cs.ennemis, vec![157]);
        // Partie classée : identité des alliés cachée -> pas de puuid inventé.
        assert!(cs.allies[0].puuid.is_none());
        assert_eq!(cs.allies[1].puuid.as_deref(), Some("moi-puuid"));
    }

    #[test]
    fn identite_masquee_jamais_gardee() {
        let session: Value = serde_json::from_str(
            r#"{"localPlayerCellId":0,"myTeam":[
                {"cellId":0,"championId":1,"puuid":"p-moi"},
                {"cellId":1,"championId":2,"puuid":"p-cache","nameVisibilityType":"HIDDEN"}]}"#,
        )
        .unwrap();
        let cs = lire(&session).unwrap();
        assert_eq!(cs.allies[0].puuid.as_deref(), Some("p-moi"));
        assert!(cs.allies[1].puuid.is_none());
    }

    #[test]
    fn survol_lu_dans_les_actions() {
        // myTeam ne donne rien (0), le survol n'est que dans l'action de pick en cours.
        let session: Value = serde_json::from_str(
            r#"{"localPlayerCellId":0,"myTeam":[{"cellId":0,"championId":0,"championPickIntent":0}],
                "actions":[[{"actorCellId":0,"type":"pick","completed":false,"championId":157}]]}"#,
        )
        .unwrap();
        let cs = lire(&session).unwrap();
        assert_eq!((cs.champion, cs.verrouille), (157, false));
    }

    #[test]
    fn survol_non_verrouille() {
        let session: Value = serde_json::from_str(
            r#"{"localPlayerCellId":0,"myTeam":[{"cellId":0,"championId":0,"championPickIntent":103}],
                "actions":[[{"actorCellId":0,"type":"pick","completed":false}]]}"#,
        )
        .unwrap();
        let cs = lire(&session).unwrap();
        assert_eq!((cs.champion, cs.verrouille), (103, false));
    }
}
