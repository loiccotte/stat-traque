//! Thread "client LoL" : trouve le client, ouvre un WebSocket et transforme
//! ce que le client nous pousse en `Evenement`.
//!
//! Pourquoi un WebSocket ? Avant, on demandait au client « quel champion ? »
//! toutes les secondes, même quand on ne jouait pas. Maintenant le client nous
//! prévient lui-même : entre deux événements, ce thread dort (0 % CPU).
//!
//! Protocole (WAMP, très simple) :
//!   on envoie   [5, "OnJsonApiEvent_lol-gameflow_v1_gameflow-phase"]   (= s'abonner)
//!   on reçoit   [8, "OnJsonApiEvent_...", {"uri": "...", "eventType": "Update", "data": ...}]

use std::net::TcpStream;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::{Connector, Message};

use super::{trouver_client, Lcu};
use crate::evenement::Evenement;
use crate::gameflow::Phase;
use crate::log;

/// Les sujets auxquels on s'abonne. Ajouter ici les futurs besoins (runes, lobby...).
const ABONNEMENTS: [&str; 2] = [
    "OnJsonApiEvent_lol-gameflow_v1_gameflow-phase",
    "OnJsonApiEvent_lol-champ-select_v1_session",
];

/// Toutes les combien de tentatives on se permet l'appel PowerShell (coûteux).
/// Tentative toutes les 3 s -> PowerShell au plus toutes les 30 s.
const TENTATIVES_AVANT_POWERSHELL: u32 = 10;

/// Lance le thread. Il tourne jusqu'à la fin du programme.
pub fn lancer(tx: Sender<Evenement>) {
    thread::Builder::new()
        .name("client-lol".into())
        .stack_size(256 * 1024) // petite pile : ce thread ne fait pas de récursion
        .spawn(move || boucle(tx))
        .expect("impossible de lancer le thread client LoL");
}

fn boucle(tx: Sender<Evenement>) {
    let mut tentative: u32 = 0;
    loop {
        let reglages = crate::stockage::Reglages::charger();
        let avec_powershell = tentative.is_multiple_of(TENTATIVES_AVANT_POWERSHELL);
        tentative += 1;

        let Some((lcu, dossier)) = trouver_client(reglages.get("dossier_lol"), avec_powershell) else {
            thread::sleep(Duration::from_secs(3));
            continue;
        };
        memoriser_dossier(reglages, &dossier);

        match ouvrir_websocket(&lcu) {
            Ok(socket) => {
                tentative = 0;
                if tx.send(Evenement::ClientConnecte).is_err() {
                    return; // le cerveau est arrêté : on s'arrête aussi
                }
                etat_initial(&lcu, &tx);
                ecouter(socket, &tx);
                if tx.send(Evenement::ClientPerdu).is_err() {
                    return;
                }
            }
            // Le lockfile existe mais le client démarre encore : on réessaie bientôt.
            Err(e) => log!("Client LoL pas encore prêt ({e})"),
        }
        thread::sleep(Duration::from_secs(3));
    }
}

/// On mémorise le dossier du jeu : la prochaine fois, plus besoin de PowerShell.
fn memoriser_dossier(mut reglages: crate::stockage::Reglages, dossier: &std::path::Path) {
    let texte = dossier.to_string_lossy();
    if reglages.get("dossier_lol") != Some(&texte) {
        reglages.set("dossier_lol", &texte);
        let _ = reglages.sauver();
    }
}

type Socket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

fn ouvrir_websocket(lcu: &Lcu) -> Result<Socket, Box<dyn std::error::Error>> {
    let mut requete = format!("wss://127.0.0.1:{}", lcu.port).into_client_request()?;
    requete.headers_mut().insert("Authorization", lcu.auth.parse()?);
    let tcp = TcpStream::connect(("127.0.0.1", lcu.port))?;
    let connecteur = Connector::NativeTls(crate::http::tls_local());
    let (mut socket, _) = tungstenite::client_tls_with_config(requete, tcp, None, Some(connecteur))?;
    for sujet in ABONNEMENTS {
        socket.send(Message::text(format!("[5, \"{sujet}\"]")))?;
    }
    Ok(socket)
}

/// Le WebSocket ne prévient que des CHANGEMENTS : on lit donc l'état actuel une fois.
fn etat_initial(lcu: &Lcu, tx: &Sender<Evenement>) {
    let phase = lcu
        .get("/lol-gameflow/v1/gameflow-phase")
        .and_then(|v| v.as_str().map(Phase::depuis_texte))
        .unwrap_or(Phase::Aucune);
    let _ = tx.send(Evenement::Phase(phase));
    if phase == Phase::ChampSelect {
        if let Some(ev) = lcu.get("/lol-champ-select/v1/session").as_ref().and_then(lire_champ_select) {
            let _ = tx.send(ev);
        }
    }
}

/// Lit les messages jusqu'à la fermeture du client. Bloquant : dort entre deux messages.
fn ecouter(mut socket: Socket, tx: &Sender<Evenement>) {
    // Le client renvoie la session de champ select à CHAQUE petit changement
    // (timer, survol d'un allié...) : on ne transmet que si MON choix change.
    let mut dernier_champ_select: Option<(u16, Option<Box<str>>)> = None;
    while let Ok(message) = socket.read() {
        let Message::Text(texte) = message else { continue };
        if let Some(ev) = traduire(&texte) {
            if let Evenement::ChampSelect { champion, role } = &ev {
                let cle = (*champion, role.clone());
                if dernier_champ_select.as_ref() == Some(&cle) {
                    continue;
                }
                dernier_champ_select = Some(cle);
            }
            if matches!(ev, Evenement::Phase(_)) {
                dernier_champ_select = None;
            }
            if tx.send(ev).is_err() {
                return;
            }
        }
    }
}

/// Transforme un message WAMP en événement (ou rien). Fonction pure : facile à tester.
pub fn traduire(texte: &str) -> Option<Evenement> {
    let v: Value = serde_json::from_str(texte).ok()?;
    if v[0].as_i64() != Some(8) {
        return None;
    }
    let charge = &v[2];
    match charge["uri"].as_str()? {
        "/lol-gameflow/v1/gameflow-phase" => {
            Some(Evenement::Phase(Phase::depuis_texte(charge["data"].as_str().unwrap_or("None"))))
        }
        "/lol-champ-select/v1/session" if charge["eventType"] != "Delete" => lire_champ_select(&charge["data"]),
        _ => None,
    }
}

/// Dans la session de champ select, retrouve MA ligne (`localPlayerCellId`).
/// `championId` = verrouillé ; `championPickIntent` = survolé (pas encore verrouillé).
fn lire_champ_select(session: &Value) -> Option<Evenement> {
    let ma_case = session["localPlayerCellId"].as_i64()?;
    let moi = session["myTeam"].as_array()?.iter().find(|j| j["cellId"].as_i64() == Some(ma_case))?;
    let champion = match moi["championId"].as_u64() {
        Some(id) if id != 0 => id,
        _ => moi["championPickIntent"].as_u64().unwrap_or(0),
    } as u16;
    let role = moi["assignedPosition"].as_str().filter(|r| !r.is_empty()).map(Box::from);
    Some(Evenement::ChampSelect { champion, role })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traduit_un_changement_de_phase() {
        let msg = r#"[8,"OnJsonApiEvent",{"data":"ChampSelect","eventType":"Update","uri":"/lol-gameflow/v1/gameflow-phase"}]"#;
        assert!(matches!(traduire(msg), Some(Evenement::Phase(Phase::ChampSelect))));
    }

    #[test]
    fn traduit_mon_champion_survole_puis_verrouille() {
        let survole = r#"[8,"OnJsonApiEvent",{"eventType":"Update","uri":"/lol-champ-select/v1/session",
            "data":{"localPlayerCellId":2,"myTeam":[
              {"cellId":1,"championId":64,"championPickIntent":0,"assignedPosition":"jungle"},
              {"cellId":2,"championId":0,"championPickIntent":238,"assignedPosition":"middle"}]}}]"#;
        match traduire(survole) {
            Some(Evenement::ChampSelect { champion, role }) => {
                assert_eq!(champion, 238);
                assert_eq!(role.as_deref(), Some("middle"));
            }
            autre => panic!("inattendu : {autre:?}"),
        }
        let verrouille = survole.replace(r#""championId":0,"championPickIntent":238"#, r#""championId":157,"championPickIntent":0"#);
        assert!(matches!(traduire(&verrouille), Some(Evenement::ChampSelect { champion: 157, .. })));
    }

    #[test]
    fn ignore_le_reste() {
        assert!(traduire("[8,\"x\",{\"uri\":\"/autre\",\"data\":1}]").is_none());
        assert!(traduire("[0,\"bienvenue\"]").is_none());
        assert!(traduire("pas du json").is_none());
        let fin = r#"[8,"x",{"eventType":"Delete","uri":"/lol-champ-select/v1/session","data":null}]"#;
        assert!(traduire(fin).is_none());
    }
}
