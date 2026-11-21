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

/// Client fermé : on regarde si le lockfile est apparu toutes les 3 s.
/// C'est UNE lecture de fichier (quelques microsecondes) : CPU ~0 %.
const ATTENTE_CLIENT: Duration = Duration::from_secs(3);
/// Si le dossier du jeu est connu, on ne parcourt la liste des processus
/// (au cas où le jeu aurait été déplacé) qu'une fois sur 20 (~1 min).
const TENTATIVES_PAR_SCAN: u32 = 20;

/// Lance le thread. Il tourne jusqu'à la fin du programme.
pub fn lancer(tx: Sender<Evenement>) {
    thread::Builder::new()
        .name("client-lol".into())
        .stack_size(256 * 1024) // petite pile : ce thread ne fait pas de récursion
        .spawn(move || boucle(tx))
        .expect("impossible de lancer le thread client LoL");
}

fn boucle(tx: Sender<Evenement>) {
    // Les réglages sont lus UNE fois (pas à chaque tentative).
    let mut reglages = crate::stockage::Reglages::charger();
    let mut tentative: u32 = 0;
    // Dernière erreur journalisée : on n'écrit que les CHANGEMENTS (journal borné).
    let mut derniere_erreur = String::new();
    // Attente entre deux échecs de connexion (lockfile périmé après un crash...) : 3 s -> 30 s.
    let mut attente_echec = ATTENTE_CLIENT;
    loop {
        let scanner = reglages.get("dossier_lol").is_none() || tentative.is_multiple_of(TENTATIVES_PAR_SCAN);
        tentative = tentative.wrapping_add(1);

        let Some((lcu, dossier)) = trouver_client(reglages.get("dossier_lol"), scanner) else {
            thread::sleep(ATTENTE_CLIENT);
            continue;
        };
        memoriser_dossier(&mut reglages, &dossier);

        match ouvrir_websocket(&lcu) {
            Ok(socket) => {
                tentative = 1;
                derniere_erreur.clear();
                attente_echec = ATTENTE_CLIENT;
                if tx.send(Evenement::ClientConnecte).is_err() {
                    return; // le cerveau est arrêté : on s'arrête aussi
                }
                let deja_envoye = etat_initial(&lcu, &tx);
                ecouter(socket, &tx, deja_envoye);
                if tx.send(Evenement::ClientPerdu).is_err() {
                    return;
                }
                thread::sleep(ATTENTE_CLIENT);
            }
            // Le lockfile existe mais le client démarre encore (ou il est périmé).
            Err(e) => {
                let texte = e.to_string();
                if texte != derniere_erreur {
                    log!("Client LoL pas encore prêt ({texte})");
                    derniere_erreur = texte;
                }
                thread::sleep(attente_echec);
                attente_echec = (attente_echec * 2).min(Duration::from_secs(30));
            }
        }
    }
}

/// On mémorise le dossier du jeu : la prochaine fois, une seule lecture de fichier suffit.
fn memoriser_dossier(reglages: &mut crate::stockage::Reglages, dossier: &std::path::Path) {
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
    let adresse = std::net::SocketAddr::from(([127, 0, 0, 1], lcu.port));
    let tcp = TcpStream::connect_timeout(&adresse, Duration::from_secs(2))?;
    // Délai max pendant la poignée de main : un client qui démarre peut accepter
    // la connexion sans jamais répondre. Ensuite : lecture bloquante sans délai.
    tcp.set_read_timeout(Some(Duration::from_secs(3)))?;
    let connecteur = Connector::NativeTls(crate::http::tls_local());
    let (mut socket, _) = tungstenite::client_tls_with_config(requete, tcp, None, Some(connecteur))?;
    for sujet in ABONNEMENTS {
        socket.send(Message::text(format!("[5, \"{sujet}\"]")))?;
    }
    // On enlève le délai sur la VRAIE socket (sous la couche TLS) : le client peut
    // rester silencieux des minutes entières, ce n'est pas une déconnexion.
    use tungstenite::stream::MaybeTlsStream;
    match socket.get_mut() {
        MaybeTlsStream::NativeTls(tls) => tls.get_mut().set_read_timeout(None)?,
        MaybeTlsStream::Plain(tcp) => tcp.set_read_timeout(None)?,
        _ => {}
    }
    Ok(socket)
}

fn noter_get(uri: &str, data: &Value) {
    if crate::enregistreur::actif() {
        crate::enregistreur::noter(serde_json::json!({"src": "get", "uri": uri, "data": data}));
    }
}

/// Clé de déduplication du champ select : (champion, rôle).
type CleChampSelect = (u16, Option<Box<str>>);

/// Le WebSocket ne prévient que des CHANGEMENTS : on lit donc l'état actuel une fois.
/// Renvoie le champ select déjà envoyé (pour ne pas l'annoncer deux fois).
fn etat_initial(lcu: &Lcu, tx: &Sender<Evenement>) -> Option<CleChampSelect> {
    let phase = lcu
        .get("/lol-gameflow/v1/gameflow-phase")
        .inspect(|v| noter_get("/lol-gameflow/v1/gameflow-phase", v))
        .and_then(|v| v.as_str().map(Phase::depuis_texte))
        .unwrap_or(Phase::Aucune);
    let _ = tx.send(Evenement::Phase(phase));
    if phase != Phase::ChampSelect {
        return None;
    }
    let session = lcu.get("/lol-champ-select/v1/session")?;
    noter_get("/lol-champ-select/v1/session", &session);
    let ev = lire_champ_select(&session)?;
    let cle = match &ev {
        Evenement::ChampSelect { champion, role } => (*champion, role.clone()),
        _ => return None,
    };
    let _ = tx.send(ev);
    Some(cle)
}

/// Le client renvoie la session de champ select à CHAQUE petit changement
/// (timer, survol d'un allié...) : on ne transmet que si MON choix change.
/// Partagé par l'écoute en direct et par le rejeu des enregistrements.
#[derive(Default)]
pub struct Filtre {
    dernier_champ_select: Option<CleChampSelect>,
}

impl Filtre {
    pub fn filtrer(&mut self, ev: Evenement) -> Option<Evenement> {
        match &ev {
            Evenement::ChampSelect { champion, role } => {
                let cle = (*champion, role.clone());
                if self.dernier_champ_select.as_ref() == Some(&cle) {
                    return None;
                }
                self.dernier_champ_select = Some(cle);
            }
            Evenement::Phase(_) => self.dernier_champ_select = None,
            _ => {}
        }
        Some(ev)
    }
}

/// Lit les messages jusqu'à la fermeture du client. Bloquant : dort entre deux messages.
fn ecouter(mut socket: Socket, tx: &Sender<Evenement>, deja_envoye: Option<CleChampSelect>) {
    let mut filtre = Filtre { dernier_champ_select: deja_envoye };
    loop {
        let message = match socket.read() {
            Ok(m) => m,
            // Simple délai écoulé : ce n'est pas une fermeture, on continue d'écouter.
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
            {
                continue
            }
            Err(e) => {
                log!("Connexion au client LoL terminée ({e})");
                return;
            }
        };
        let Message::Text(texte) = message else { continue };
        // Le texte n'est décodé qu'UNE fois, pour l'enregistreur et pour la traduction.
        let Ok(valeur) = serde_json::from_str::<Value>(&texte) else { continue };
        if crate::enregistreur::actif() {
            crate::enregistreur::noter(serde_json::json!({"src": "ws", "msg": valeur}));
        }
        if let Some(ev) = traduire(&valeur).and_then(|ev| filtre.filtrer(ev)) {
            if tx.send(ev).is_err() {
                return;
            }
        }
    }
}

/// Transforme un message WAMP (déjà décodé) en événement, ou rien.
/// Fonction pure : facile à tester.
pub fn traduire(v: &Value) -> Option<Evenement> {
    if v[0].as_i64() != Some(8) {
        return None;
    }
    let charge = &v[2];
    match charge["uri"].as_str()? {
        "/lol-gameflow/v1/gameflow-phase" => {
            let texte = charge["data"].as_str().unwrap_or("None");
            let phase = Phase::depuis_texte(texte);
            if phase == Phase::Autre {
                log!("Phase inconnue du client : {texte}");
            }
            Some(Evenement::Phase(phase))
        }
        "/lol-champ-select/v1/session" if charge["eventType"] != "Delete" => lire_champ_select(&charge["data"]),
        _ => None,
    }
}

/// Dans la session de champ select, retrouve MA ligne (`localPlayerCellId`).
/// `championId` = verrouillé ; `championPickIntent` = survolé (pas encore verrouillé).
pub fn lire_champ_select(session: &Value) -> Option<Evenement> {
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

    fn v(texte: &str) -> Value {
        serde_json::from_str(texte).unwrap()
    }

    #[test]
    fn traduit_un_changement_de_phase() {
        let msg = r#"[8,"OnJsonApiEvent",{"data":"ChampSelect","eventType":"Update","uri":"/lol-gameflow/v1/gameflow-phase"}]"#;
        assert!(matches!(traduire(&v(msg)), Some(Evenement::Phase(Phase::ChampSelect))));
    }

    #[test]
    fn traduit_mon_champion_survole_puis_verrouille() {
        let survole = r#"[8,"OnJsonApiEvent",{"eventType":"Update","uri":"/lol-champ-select/v1/session",
            "data":{"localPlayerCellId":2,"myTeam":[
              {"cellId":1,"championId":64,"championPickIntent":0,"assignedPosition":"jungle"},
              {"cellId":2,"championId":0,"championPickIntent":238,"assignedPosition":"middle"}]}}]"#;
        match traduire(&v(survole)) {
            Some(Evenement::ChampSelect { champion, role }) => {
                assert_eq!(champion, 238);
                assert_eq!(role.as_deref(), Some("middle"));
            }
            autre => panic!("inattendu : {autre:?}"),
        }
        let verrouille = survole.replace(r#""championId":0,"championPickIntent":238"#, r#""championId":157,"championPickIntent":0"#);
        assert!(matches!(traduire(&v(&verrouille)), Some(Evenement::ChampSelect { champion: 157, .. })));
    }

    #[test]
    fn ignore_le_reste() {
        assert!(traduire(&v("[8,\"x\",{\"uri\":\"/autre\",\"data\":1}]")).is_none());
        assert!(traduire(&v("[0,\"bienvenue\"]")).is_none());
        let fin = r#"[8,"x",{"eventType":"Delete","uri":"/lol-champ-select/v1/session","data":null}]"#;
        assert!(traduire(&v(fin)).is_none());
    }
}
