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
const ABONNEMENTS: [&str; 3] = [
    "OnJsonApiEvent_lol-gameflow_v1_gameflow-phase",
    "OnJsonApiEvent_lol-champ-select_v1_session",
    // Le compte connecté : prévient si le client finit de charger le joueur APRÈS
    // notre connexion, ou si un autre compte se connecte (multi-comptes).
    "OnJsonApiEvent_lol-summoner_v1_current-summoner",
];

/// « Partie trouvée ! » (le bouton Accepter). Abonnement SEULEMENT si le réglage
/// `accepter=oui` est présent : sinon on ne reçoit même pas ces messages.
const ABONNEMENT_READY_CHECK: &str = "OnJsonApiEvent_lol-matchmaking_v1_ready-check";
/// Petit délai avant d'accepter : laisse le temps de voir la fenêtre (et de refuser).
const DELAI_ACCEPTATION: Duration = Duration::from_millis(1500);

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
    // Acceptation automatique de la partie : DÉSACTIVÉE sauf `accepter=oui`.
    let accepter = reglages.get("accepter") == Some("oui");
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

        match ouvrir_websocket(&lcu, accepter) {
            Ok(socket) => {
                tentative = 1;
                derniere_erreur.clear();
                attente_echec = ATTENTE_CLIENT;
                if tx.send(Evenement::ClientConnecte(Some(lcu.clone()))).is_err() {
                    return; // le cerveau est arrêté : on s'arrête aussi
                }
                let deja_envoye = etat_initial(&lcu, &tx);
                // `then_some` : Some(&lcu) si l'acceptation auto est activée, None sinon.
                ecouter(socket, &tx, deja_envoye, accepter.then_some(&lcu));
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

fn ouvrir_websocket(lcu: &Lcu, accepter: bool) -> Result<Socket, Box<dyn std::error::Error>> {
    let mut requete = format!("wss://127.0.0.1:{}", lcu.port).into_client_request()?;
    requete.headers_mut().insert("Authorization", lcu.auth.parse()?);
    let adresse = std::net::SocketAddr::from(([127, 0, 0, 1], lcu.port));
    let tcp = TcpStream::connect_timeout(&adresse, Duration::from_secs(2))?;
    // Délai max pendant la poignée de main : un client qui démarre peut accepter
    // la connexion sans jamais répondre. Ensuite : lecture bloquante sans délai.
    tcp.set_read_timeout(Some(Duration::from_secs(3)))?;
    let connecteur = Connector::NativeTls(crate::http::tls_local());
    let (mut socket, _) = tungstenite::client_tls_with_config(requete, tcp, None, Some(connecteur))?;
    // `chain` + `filter` : ajoute le ready-check à la liste seulement si `accepter`.
    let ready_check = std::iter::once(ABONNEMENT_READY_CHECK).filter(|_| accepter);
    for sujet in ABONNEMENTS.into_iter().chain(ready_check) {
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

/// Dernier champ select transmis (pour ne pas renvoyer deux fois le même).
type CleChampSelect = crate::champselect::ChampSelect;

/// Le WebSocket ne prévient que des CHANGEMENTS : on lit donc l'état actuel une fois.
/// Renvoie le champ select déjà envoyé (pour ne pas l'annoncer deux fois).
fn etat_initial(lcu: &Lcu, tx: &Sender<Evenement>) -> Option<CleChampSelect> {
    if let Some(moi) = lcu.get("/lol-summoner/v1/current-summoner") {
        noter_get("/lol-summoner/v1/current-summoner", &moi);
        if let Some(puuid) = moi["puuid"].as_str().filter(|p| !p.is_empty()) {
            let summoner_id = moi["summonerId"].as_u64().filter(|&id| id != 0);
            let _ = tx.send(Evenement::Moi { puuid: puuid.into(), summoner_id });
        }
    }
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
    let cs = crate::champselect::lire(&session)?;
    let _ = tx.send(Evenement::ChampSelect(cs.clone()));
    Some(cs)
}

/// Le client renvoie la session de champ select à CHAQUE petit changement
/// (le timer, chaque seconde) : on ne transmet que si les CHOIX changent.
/// Partagé par l'écoute en direct et par le rejeu des enregistrements.
#[derive(Default)]
pub struct Filtre {
    dernier_champ_select: Option<CleChampSelect>,
}

impl Filtre {
    pub fn filtrer(&mut self, ev: Evenement) -> Option<Evenement> {
        match &ev {
            Evenement::ChampSelect(cs) => {
                if self.dernier_champ_select.as_ref() == Some(cs) {
                    return None;
                }
                self.dernier_champ_select = Some(cs.clone());
            }
            Evenement::Phase(_) => self.dernier_champ_select = None,
            _ => {}
        }
        Some(ev)
    }
}

/// Lit les messages jusqu'à la fermeture du client. Bloquant : dort entre deux messages.
/// `accepter` : le client à qui envoyer « Accepter » (None = acceptation auto désactivée).
fn ecouter(mut socket: Socket, tx: &Sender<Evenement>, deja_envoye: Option<CleChampSelect>, accepter: Option<&Lcu>) {
    let mut filtre = Filtre { dernier_champ_select: deja_envoye };
    let mut acceptation = AutoAcceptation::default();
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
        if let Some(lcu) = accepter {
            if acceptation.observer(&valeur) {
                accepter_bientot(lcu.clone());
            }
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
        // Compte connecté (nouveau compte, ou joueur chargé après notre connexion).
        "/lol-summoner/v1/current-summoner" if charge["eventType"] != "Delete" => {
            let puuid = charge["data"]["puuid"].as_str().filter(|p| !p.is_empty())?;
            let summoner_id = charge["data"]["summonerId"].as_u64().filter(|&id| id != 0);
            Some(Evenement::Moi { puuid: puuid.into(), summoner_id })
        }

        _ => None,
    }
}

/// Session de champ select -> événement.
pub fn lire_champ_select(session: &Value) -> Option<Evenement> {
    crate::champselect::lire(session).map(Evenement::ChampSelect)
}

// ---------- Acceptation automatique de la partie (réglage `accepter=oui`) ----------
//
// Conformité Riot : c'est une action dans le CLIENT, AVANT la partie (comme Blitz,
// Porofessor...), jamais dans le jeu. Désactivée par défaut.

/// Ce que dit un message WAMP sur la fenêtre « Partie trouvée ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyCheck {
    /// La fenêtre est ouverte et je n'ai pas encore répondu.
    AAccepter,
    /// Ouverte, mais j'ai déjà répondu (accepté ou refusé) : on ne touche à rien.
    DejaRepondu,
    /// Terminée (tout le monde a accepté, quelqu'un a refusé, file annulée...).
    Terminee,
}

/// État de la vérification à partir de ses données (`data`). Fonction pure.
/// Exemple de données : {"state":"InProgress","playerResponse":"None","timer":3.0}
pub fn etat_ready_check(data: &Value) -> ReadyCheck {
    match (data["state"].as_str(), data["playerResponse"].as_str()) {
        (Some("InProgress"), Some("None")) => ReadyCheck::AAccepter,
        (Some("InProgress"), _) => ReadyCheck::DejaRepondu,
        _ => ReadyCheck::Terminee,
    }
}

/// Message WAMP -> état du ready-check, ou `None` si le message parle d'autre chose.
pub fn lire_ready_check(v: &Value) -> Option<ReadyCheck> {
    let charge = &v[2];
    if v[0].as_i64() != Some(8) || charge["uri"] != "/lol-matchmaking/v1/ready-check" {
        return None;
    }
    if charge["eventType"] == "Delete" {
        return Some(ReadyCheck::Terminee);
    }
    Some(etat_ready_check(&charge["data"]))
}

/// Le client renvoie le ready-check CHAQUE seconde (le compte à rebours) :
/// on ne programme l'acceptation qu'UNE fois par fenêtre « Partie trouvée ».
#[derive(Default)]
pub struct AutoAcceptation {
    programmee: bool,
}

impl AutoAcceptation {
    /// `true` = il faut programmer l'acceptation maintenant.
    pub fn observer(&mut self, v: &Value) -> bool {
        match lire_ready_check(v) {
            // `replace` met `true` et renvoie l'ANCIENNE valeur : vrai la 1re fois seulement.
            Some(ReadyCheck::AAccepter) => !std::mem::replace(&mut self.programmee, true),
            // Fenêtre fermée : la prochaine « Partie trouvée » pourra être acceptée.
            Some(ReadyCheck::Terminee) => {
                self.programmee = false;
                false
            }
            _ => false,
        }
    }
}

/// Accepte la partie après `DELAI_ACCEPTATION`, dans un petit thread jetable
/// (le thread d'écoute ne doit pas dormir : il raterait des messages).
fn accepter_bientot(lcu: Lcu) {
    let _ = thread::Builder::new()
        .name("acceptation".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            thread::sleep(DELAI_ACCEPTATION);
            // On revérifie : le joueur a peut-être accepté ou refusé lui-même entre-temps.
            let encore = lcu
                .get("/lol-matchmaking/v1/ready-check")
                .is_some_and(|rc| etat_ready_check(&rc) == ReadyCheck::AAccepter);
            if !encore {
                return;
            }
            match lcu.envoyer("POST", "/lol-matchmaking/v1/ready-check/accept", None) {
                Ok(()) => log!("Partie acceptée automatiquement (réglage accepter=oui)."),
                Err(e) => log!("Acceptation automatique impossible : {e}"),
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traduit_le_compte_connecte() {
        let msg = r#"[8,"x",{"eventType":"Update","uri":"/lol-summoner/v1/current-summoner","data":{"puuid":"abc","summonerId":42}}]"#;
        assert!(matches!(traduire(&v(msg)), Some(Evenement::Moi { puuid, summoner_id: Some(42) }) if &*puuid == "abc"));
        // summonerId absent ou 0 (anonymisé) -> None.
        let sans = r#"[8,"x",{"eventType":"Update","uri":"/lol-summoner/v1/current-summoner","data":{"puuid":"abc","summonerId":0}}]"#;
        assert!(matches!(traduire(&v(sans)), Some(Evenement::Moi { summoner_id: None, .. })));
        // Joueur pas encore chargé : puuid vide -> rien.
        let vide = r#"[8,"x",{"eventType":"Update","uri":"/lol-summoner/v1/current-summoner","data":{"puuid":""}}]"#;
        assert!(traduire(&v(vide)).is_none());
    }

    fn ready_check(event: &str, etat: &str, reponse: &str) -> Value {
        v(&format!(
            r#"[8,"x",{{"eventType":"{event}","uri":"/lol-matchmaking/v1/ready-check",
                "data":{{"state":"{etat}","playerResponse":"{reponse}","timer":2.0}}}}]"#
        ))
    }

    #[test]
    fn lit_le_ready_check() {
        assert_eq!(lire_ready_check(&ready_check("Create", "InProgress", "None")), Some(ReadyCheck::AAccepter));
        assert_eq!(lire_ready_check(&ready_check("Update", "InProgress", "Accepted")), Some(ReadyCheck::DejaRepondu));
        assert_eq!(lire_ready_check(&ready_check("Update", "InProgress", "Declined")), Some(ReadyCheck::DejaRepondu));
        assert_eq!(lire_ready_check(&ready_check("Update", "EveryoneReady", "Accepted")), Some(ReadyCheck::Terminee));
        assert_eq!(lire_ready_check(&ready_check("Delete", "InProgress", "None")), Some(ReadyCheck::Terminee));
        // Un autre sujet n'est pas un ready-check.
        let phase = r#"[8,"x",{"data":"Lobby","eventType":"Update","uri":"/lol-gameflow/v1/gameflow-phase"}]"#;
        assert_eq!(lire_ready_check(&v(phase)), None);
    }

    #[test]
    fn accepte_une_seule_fois_par_partie_trouvee() {
        let mut a = AutoAcceptation::default();
        assert!(a.observer(&ready_check("Create", "InProgress", "None")));
        // Le compte à rebours renvoie le même état chaque seconde : pas de 2e acceptation.
        assert!(!a.observer(&ready_check("Update", "InProgress", "None")));
        assert!(!a.observer(&ready_check("Update", "InProgress", "Accepted")));
        // Quelqu'un refuse, retour en file, nouvelle partie trouvée : on accepte à nouveau.
        assert!(!a.observer(&ready_check("Update", "StrangerNotReady", "Accepted")));
        assert!(!a.observer(&ready_check("Delete", "Invalid", "None")));
        assert!(a.observer(&ready_check("Create", "InProgress", "None")));
        // Si j'ai déjà refusé moi-même : rien.
        let mut b = AutoAcceptation::default();
        assert!(!b.observer(&ready_check("Create", "InProgress", "Declined")));
    }

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
            Some(Evenement::ChampSelect(cs)) => {
                assert_eq!(cs.champion, 238);
                assert_eq!(cs.role.as_deref(), Some("middle"));
            }
            autre => panic!("inattendu : {autre:?}"),
        }
        let verrouille = survole.replace(r#""championId":0,"championPickIntent":238"#, r#""championId":157,"championPickIntent":0"#);
        assert!(matches!(traduire(&v(&verrouille)), Some(Evenement::ChampSelect(cs)) if cs.champion == 157));
    }

    #[test]
    fn ignore_le_reste() {
        assert!(traduire(&v("[8,\"x\",{\"uri\":\"/autre\",\"data\":1}]")).is_none());
        assert!(traduire(&v("[0,\"bienvenue\"]")).is_none());
        let fin = r#"[8,"x",{"eventType":"Delete","uri":"/lol-champ-select/v1/session","data":null}]"#;
        assert!(traduire(&v(fin)).is_none());
    }
}
