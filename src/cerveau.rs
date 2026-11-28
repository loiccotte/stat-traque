//! Le "cerveau" : reçoit TOUS les événements et décide quoi faire.
//! Un seul thread possède l'état de l'app -> pas de verrous, pas de course.
//!
//! En champ select : analyse des compos, picks conseillés, runes + sorts (stats)
//! appliqués au verrouillage, rangs des alliés si le client les donne.
//! En partie : rangs des 10 joueurs, tableau de la partie (console).

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use crate::champselect::ChampSelect;
use crate::ddragon::DonneesJeu;
use crate::evenement::Evenement;
use crate::gameflow::Phase;
use crate::lcu::Lcu;
use crate::live::{self, InstantanePartie, SuiviPartie};
use crate::rangs::InfoJoueur;
use crate::riot_api::RiotApi;
use crate::affichage::{self, VueChampSelect, VueRunes};
use crate::{compo, log, stats};

/// Comment l'app tourne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Icône dans la barre des tâches (normal).
    Tray,
    /// Messages et tableau de la partie dans une console.
    Console,
    /// Rejeu d'un enregistrement : console, mais on n'agit pas sur un vrai client.
    Rejeu,
}

/// Ce qu'on a déjà fait pendant ce champ select (pour ne rien faire deux fois).
#[derive(Default)]
struct SuiviChampSelect {
    dernier: Option<ChampSelect>,
    champion_annonce: u16,
    runes_pour: u16,
    compo_affichee: (Vec<u16>, Vec<u16>),
    rangs_demandes: bool,
    /// La file a déjà été relue pendant ce champ select (un seul essai).
    file_relue: bool,
    /// Matchup / contre-picks déjà calculés pour (mon champion, ennemis, verrouillé).
    lane_pour: (u16, Vec<u16>, bool),
    /// Meilleurs / pires matchups déjà annoncés pour ce champion.
    matchups_pour: u16,
    /// Stats lues pour ces champions (mon champion + ennemis visibles) : relues
    /// seulement quand cette liste change. Quelques dizaines de Ko au plus.
    stats_lues: Option<(Vec<u16>, stats::Stats)>,
    /// (mon champion, adversaire) du dernier matchup écrit au journal.
    matchup_annonce: (u16, u16),
}

struct Etat {
    phase: Phase,
    mode: Mode,
    donnees: Option<Arc<DonneesJeu>>,
    lcu: Option<Lcu>,
    api: Option<Arc<RiotApi>>,
    mon_puuid: Option<Box<str>>,
    /// Mon puuid au format de l'API Riot (78 caractères). Le client LoL en donne un plus
    /// court (36 caractères) que Riot refuse (erreur 400) : on le convertit via account-v1.
    mon_puuid_riot: Option<Box<str>>,
    /// Une conversion client -> Riot tourne dans un thread (une seule à la fois).
    conversion_en_cours: bool,
    /// La synchro de l'historique au démarrage a été lancée pour ce compte.
    synchro_initiale_faite: bool,
    /// À lancer dès que mon puuid Riot sera connu (fin de partie, « Ma fiche »).
    synchro_apres_partie_attendue: bool,
    fiche_moi_attendue: bool,
    cs: SuiviChampSelect,
    /// Champ select reçu avant que Data Dragon soit prêt : traité après.
    cs_en_attente: Option<ChampSelect>,
    partie_scoutee: bool,
    /// File de la partie lue dans /lol-gameflow/v1/session (source locale, au début de
    /// la partie) : permet le récap même si spectator-v5 a échoué.
    file_gameflow: Option<u16>,
    /// Arena (CHERRY) : pas d'équipes fiables à afficher.
    sans_equipes: bool,
    /// Numéro du compte connecté : +1 à chaque changement de compte. Les recherches de
    /// rangs le reçoivent et le renvoient ; un résultat d'un ancien numéro est ignoré.
    session_compte: u32,
    /// Existe seulement pendant une partie (le lâcher arrête le thread).
    suivi: Option<SuiviPartie>,
    /// Mon summonerId (lu au client) : sert aux item sets.
    mon_summoner_id: Option<u64>,
}

pub fn boucle(rx: Receiver<Evenement>, tx: Sender<Evenement>, mode: Mode) {
    let reglages = crate::stockage::Reglages::charger();
    // Sans clé API : pas de rangs, mais tout le reste marche.
    let api = (mode != Mode::Rejeu && crate::stockage::lire_cle().is_some())
        .then(|| Arc::new(RiotApi::depuis_reglages(&reglages)));
    let mut etat = Etat {
        phase: Phase::Aucune,
        mode,
        donnees: None,
        lcu: None,
        api,
        mon_puuid: None,
        mon_puuid_riot: None,
        conversion_en_cours: false,
        synchro_initiale_faite: false,
        synchro_apres_partie_attendue: false,
        fiche_moi_attendue: false,
        cs: SuiviChampSelect::default(),
        cs_en_attente: None,
        partie_scoutee: false,
        file_gameflow: None,
        sans_equipes: false,
        session_compte: 0,
        suivi: None,
        mon_summoner_id: None,
    };
    crate::tray::definir_etat("En attente du client LoL");
    let cle = etat.api.is_some();
    affichage::modifier(|e| e.cle_api = cle);
    // `recv()` bloque sans consommer de CPU jusqu'au prochain événement.
    while let Ok(ev) = rx.recv() {
        match ev {
            Evenement::ClientConnecte(lcu) => {
                log!("Client LoL connecté.");
                etat.lcu = lcu;
                affichage::modifier(|e| e.client_connecte = true);
                crate::tray::definir_etat("Client LoL connecté");
            }
            Evenement::ClientPerdu => {
                log!("Client LoL fermé, en attente...");
                // On NE coupe PAS le suivi de partie : le jeu continue sans le client
                // (beaucoup ferment le client pendant la partie). Il s'arrête seul à la fin.
                etat.lcu = None;
                affichage::modifier(|e| e.client_connecte = false);
                crate::tray::definir_etat("En attente du client LoL");
            }
            Evenement::Moi { puuid, summoner_id } => changer_compte(&mut etat, puuid, summoner_id, &tx),
            Evenement::Phase(p) => changer_phase(&mut etat, p, &tx),
            Evenement::ChampSelect(cs) => match etat.donnees.clone() {
                Some(d) => champ_select(&mut etat, &d, cs, &tx),
                None => etat.cs_en_attente = Some(cs),
            },
            Evenement::DonneesPretes(d) => {
                preparer_stats_en_fond(&d);
                etat.donnees = Some(d.clone());
                let partage = d.clone();
                affichage::modifier(|e| e.donnees = Some(partage)); // noms pour l'interface
                if let Some(cs) = etat.cs_en_attente.take() {
                    champ_select(&mut etat, &d, cs, &tx);
                }
            }
            // Recherche lancée pour un compte précédent : résultat périmé, ignoré.
            Evenement::Rangs { session, .. } if session != etat.session_compte => {}
            Evenement::Rangs { joueurs, .. } => {
                noter_sans_equipes(&mut etat, None);
                if let Some(d) = &etat.donnees {
                    afficher_rangs(d, &joueurs, etat.sans_equipes);
                }
                affichage::modifier(|e| e.rangs = joueurs);
            }
            Evenement::Partie(p) => {
                noter_sans_equipes(&mut etat, Some(&p.mode));
                if etat.mode != Mode::Tray {
                    afficher_partie(&p, etat.sans_equipes);
                }
                // Mes chiffres pour la petite fenêtre : lus dans l'instantané déjà reçu
                // (aucune requête de plus). Mon champion vient du champ select.
                let vue = mon_champion(&etat).and_then(|nom| p.vue_moi(&nom, None));
                affichage::modifier(|e| {
                    e.live_moi = vue;
                    e.partie = Some(p);
                });
            }
            Evenement::Historique(maj) => crate::historique::publier(*maj, etat.donnees.as_deref()),
            Evenement::DemandeFiche { puuid } => demander_fiche(&etat, &puuid, &tx),
            Evenement::DemandeMaFiche => match puuid_pour_api(&mut etat, &tx) {
                Some(moi) => demander_fiche(&etat, &moi, &tx),
                // Jamais le puuid court : l'historique est rangé sous le puuid Riot.
                None if etat.mon_puuid.is_none() => log!("Ma fiche : compte inconnu (lance le client LoL une fois)."),
                None => {
                    etat.fiche_moi_attendue = true;
                    log!("Ma fiche : compte pas encore identifié auprès de Riot (une clé API valide est nécessaire la première fois).");
                }
            },
            Evenement::MonPuuidRiot { session, puuid } => {
                if session == etat.session_compte {
                    etat.conversion_en_cours = false;
                    if let Some(p) = puuid {
                        if let Some(client) = &etat.mon_puuid {
                            crate::lcu::identite::memoire_ecrire(client, &p);
                        }
                        etat.mon_puuid_riot = Some(p);
                        apres_identification(&mut etat, &tx);
                    } else {
                        // Échec : la fiche demandée n'est pas ouverte bien plus tard par surprise
                        // (la synchro de fin de partie, elle, reste en attente).
                        etat.fiche_moi_attendue = false;
                    }
                }
            }
            Evenement::Fiche(f) => crate::fiche::publier(*f, etat.donnees.as_deref()),
            Evenement::Fenetre => suivre_si_utile(&mut etat, &tx),
            Evenement::Quitter => break,
        }
    }
}

fn changer_phase(etat: &mut Etat, p: Phase, tx: &Sender<Evenement>) {
    if p == etat.phase {
        return;
    }
    log!("Phase : {}", p.libelle());
    crate::tray::definir_etat(p.libelle());
    if p == Phase::ChampSelect {
        etat.cs = SuiviChampSelect::default();
        etat.partie_scoutee = false;
        etat.sans_equipes = false;
        // File du champ select (1 requête locale) : sert au garde-fou « classée ».
        etat.file_gameflow = lire_file(etat);
        crate::rangs::oublier_partie();
        // Les joueurs de la partie PRÉCÉDENTE ne sont plus « de la partie » : on retire
        // leur autorisation de fiche (on la garde après la partie pour consulter leur fiche).
        crate::fiche::autoriser(Vec::new());
        affichage::modifier(|e| {
            e.champ_select = Some(VueChampSelect::default());
            e.rangs.clear();
            e.sans_equipes = false;
            e.partie = None;
        });
    }
    // Hors partie (fin, salon, champ select...) : mes chiffres en direct sont périmés.
    let hors_partie = !p.en_partie();
    affichage::modifier(|e| {
        e.phase = Some(p);
        if hors_partie {
            e.live_moi = None;
        }
    });
    // Retour au salon / client au repos : la partie précédente est finie (une partie
    // TFT, sans champ select, ne doit pas hériter de la file ni du mode Arena).
    if matches!(p, Phase::Lobby | Phase::Aucune) {
        etat.file_gameflow = None;
        etat.sans_equipes = false;
    }
    // File de la partie, lue une fois (1 requête locale, ~0 coût) : au début de la
    // partie, ou en fin de partie si le client était fermé au démarrage.
    let fin = matches!(p, Phase::WaitingForStats | Phase::PreEndOfGame | Phase::EndOfGame);
    if (p.en_partie() || fin) && etat.file_gameflow.is_none() {
        etat.file_gameflow = lire_file(etat);
    }
    // Fin de partie : on récupère la partie chez Riot (historique + récap + LP).
    // Synchro seulement pour une partie officielle (une personnalisée ou l'outil
    // d'entraînement n'apparaîtront jamais dans match-v5 : requêtes inutiles).
    // Spectator OU le client : une seule des deux sources suffit.
    let file_connue = crate::rangs::file_partie().or(etat.file_gameflow);
    if p == Phase::EndOfGame && file_connue.is_some() && etat.api.is_some() {
        match puuid_pour_api(etat, tx) {
            Some(riot) => crate::historique::lancer_synchro(etat.api.clone().unwrap(), riot, true, tx.clone()),
            // Compte pas encore identifié : la synchro partira dès qu'il le sera.
            None => etat.synchro_apres_partie_attendue = true,
        }
    }
    scouter_si_besoin(etat, p, tx);
    etat.phase = p;
    suivre_si_utile(etat, tx);
}

/// Nom de mon champion (choisi en champ select), pour me retrouver dans la liste du jeu.
/// `None` si inconnu (client lancé en cours de partie, Data Dragon pas prêt...).
fn mon_champion(etat: &Etat) -> Option<String> {
    let cs = etat.cs.dernier.as_ref().filter(|c| c.champion != 0)?;
    Some(etat.donnees.as_ref()?.nom_champion(cs.champion).to_string())
}

/// Le suivi de partie (port 2999) ne tourne QUE pendant la partie, et seulement
/// si quelqu'un s'en sert : la console, l'enregistreur, ou la fenêtre ouverte
/// (onglet Partie). L'overlay n'en a pas besoin (il montre les rangs).
/// En rejeu, les instantanés viennent du fichier, pas du jeu.
/// Rappelée à chaque changement de phase ET quand la fenêtre s'ouvre / se ferme.
fn suivre_si_utile(etat: &mut Etat, tx: &Sender<Evenement>) {
    let utile = match etat.mode {
        Mode::Console => true,
        Mode::Tray => crate::enregistreur::actif() || crate::ui::fenetre_ouverte(),
        Mode::Rejeu => false,
    };
    let vivant = etat.suivi.as_ref().is_some_and(SuiviPartie::vivant);
    if etat.phase.en_partie() && utile {
        if !vivant {
            etat.suivi = Some(live::suivre(tx.clone()));
        }
    } else {
        etat.suivi = None; // lâcher le suivi arrête son thread
    }
}

/// Fiche joueur à la demande (F44). Conformité Riot : seulement moi, ou un joueur
/// de ma partie en cours connu par spectator-v5 (jamais un pseudo caché de champ select).
fn demander_fiche(etat: &Etat, puuid: &str, tx: &Sender<Evenement>) {
    let patch = crate::fiche::patch_de(etat.donnees.as_deref());
    if etat.mon_puuid_riot.as_deref() == Some(puuid) || etat.mon_puuid.as_deref() == Some(puuid) {
        // Ma fiche : base locale, 0 requête -> marche aussi sans clé API.
        crate::fiche::lancer(etat.api.clone(), puuid.to_string(), "Moi".into(), true, patch, tx.clone());
    } else if let Some(nom) = crate::fiche::nom_autorise(puuid) {
        if etat.api.is_none() {
            log!("Fiche de {nom} : pas de clé API (stat-traque cle RGAPI-xxxx).");
            return;
        }
        log!("Fiche de {nom} demandée (11 requêtes au plus)...");
        crate::fiche::lancer(etat.api.clone(), puuid.to_string(), nom, false, patch, tx.clone());
    } else {
        log!("Fiche refusée : joueur inconnu (seulement les joueurs de ta partie en cours).");
    }
}

/// Rangs des 10 joueurs, une fois par partie (il faut connaître mon compte).
fn scouter_si_besoin(etat: &mut Etat, p: Phase, tx: &Sender<Evenement>) {
    if p.en_partie() && !etat.partie_scoutee {
        if let (Some(api), Some(riot)) = (etat.api.clone(), puuid_pour_api(etat, tx)) {
            etat.partie_scoutee = true;
            log!("Recherche des rangs des joueurs...");
            crate::rangs::scouter_partie(api, riot.to_string(), etat.session_compte, tx.clone());
        }
    }
}

/// File de la partie selon le client (/lol-gameflow/v1/session). 1 requête locale.
fn lire_file(etat: &Etat) -> Option<u16> {
    etat.lcu.as_ref().and_then(|l| l.get("/lol-gameflow/v1/session")).and_then(|s| crate::gameflow::file_depuis_session(&s))
}

/// Mon puuid au format de l'API Riot (voir lcu/identite.rs : le client donne un puuid
/// de 36 caractères, Riot en attend 78). NE BLOQUE JAMAIS le cerveau :
///  - déjà connu (mémoire, ou fichier comptes.tsv) -> renvoyé tout de suite ;
///  - sinon une conversion est lancée dans un thread (une seule à la fois) et on
///    renvoie None ; le résultat arrive en `Evenement::MonPuuidRiot`, qui relance
///    ce qui attendait (synchro, rangs, ma fiche) dans `apres_identification`.
fn puuid_pour_api(etat: &mut Etat, tx: &Sender<Evenement>) -> Option<Box<str>> {
    if let Some(deja) = &etat.mon_puuid_riot {
        return Some(deja.clone());
    }
    let client = etat.mon_puuid.clone()?;
    let connu = if crate::lcu::identite::est_puuid_riot(&client) {
        Some(client.to_string()) // déjà au format Riot (rejeu, ancien client)
    } else {
        crate::lcu::identite::memoire_lire(&client) // 0 requête, marche sans clé API
    };
    if let Some(riot) = connu {
        etat.mon_puuid_riot = Some(riot.into());
        return etat.mon_puuid_riot.clone();
    }
    if !etat.conversion_en_cours {
        if let (Some(lcu), Some(api)) = (etat.lcu.clone(), etat.api.clone()) {
            etat.conversion_en_cours = true;
            let (session, tx) = (etat.session_compte, tx.clone());
            std::thread::spawn(move || {
                let source = crate::lcu::identite::Reelle { lcu: &lcu, api: &api };
                let puuid = match crate::lcu::identite::convertir(&source, &client) {
                    Ok(id) => Some(id.puuid.into()),
                    Err(e) => {
                        log!("Impossible de convertir mon puuid pour l'API Riot : {e} (nouvel essai au prochain besoin).");
                        None
                    }
                };
                let _ = tx.send(Evenement::MonPuuidRiot { session, puuid });
            });
        }
    }
    None
}

/// Mon puuid Riot vient d'être connu : on lance tout ce qui l'attendait.
fn apres_identification(etat: &mut Etat, tx: &Sender<Evenement>) {
    let Some(riot) = etat.mon_puuid_riot.clone() else { return };
    if let Some(api) = etat.api.clone() {
        if !etat.synchro_initiale_faite {
            etat.synchro_initiale_faite = true;
            crate::historique::lancer_synchro(api.clone(), riot.clone(), false, tx.clone());
        }
        if std::mem::take(&mut etat.synchro_apres_partie_attendue) {
            crate::historique::lancer_synchro(api, riot.clone(), true, tx.clone());
        }
    }
    if std::mem::take(&mut etat.fiche_moi_attendue) {
        demander_fiche(etat, &riot, tx);
    }
    let phase = etat.phase;
    scouter_si_besoin(etat, phase, tx);
}

/// Multi-comptes : le client annonce son compte à chaque connexion (et dès qu'il
/// a fini de le charger). Si c'est un AUTRE compte qu'avant (déconnexion puis
/// connexion d'un ami sur le même PC...), tout ce qui dépend du joueur repart de zéro.
fn changer_compte(etat: &mut Etat, puuid: Box<str>, summoner_id: Option<u64>, tx: &Sender<Evenement>) {
    // summonerId (item sets) : toujours celui du compte annoncé. Pour le même compte,
    // un événement sans summonerId ne fait pas oublier celui déjà connu.
    if summoner_id.is_some() || etat.mon_puuid.as_deref() != Some(&*puuid) {
        etat.mon_summoner_id = summoner_id;
    }
    if let Some(ancien) = &etat.mon_puuid {
        if *ancien == puuid {
            return; // même compte (reconnexion, changement d'icône...) : rien à refaire
        }
        // Un compte différent : on oublie ce qui a été fait pour l'ancien.
        log!("Autre compte connecté au client LoL : on repart de zéro.");
        crate::fiche::autoriser(Vec::new());
        // `wrapping_add` : repasse à 0 après u32::MAX au lieu de planter (jamais atteint en pratique).
        etat.session_compte = etat.session_compte.wrapping_add(1);
        etat.cs = SuiviChampSelect::default();
        etat.cs_en_attente = None;
        etat.partie_scoutee = false;
        etat.file_gameflow = None;
        etat.sans_equipes = false;
        affichage::modifier(|e| {
            e.sans_equipes = false;
            e.champ_select = None;
            e.rangs.clear();
            e.partie = None;
            e.live_moi = None;
            e.fiche = None;
        });
    }
    // Premier compte vu : rien à oublier (le champ select en cours reste valable).
    // Nouveau compte (ou premier) : synchro de son historique en arrière-plan.
    // (Pas à chaque `Moi` : le client en renvoie un à chaque changement d'XP, d'icône...)
    etat.mon_puuid = Some(puuid);
    etat.mon_puuid_riot = None; // à retrouver pour ce compte
    etat.conversion_en_cours = false; // une conversion de l'ancien compte sera ignorée (session)
    etat.synchro_initiale_faite = false;
    etat.synchro_apres_partie_attendue = false;
    etat.fiche_moi_attendue = false;
    // Connu tout de suite (fichier comptes.tsv) : synchro + rangs maintenant ; sinon
    // la conversion part en arrière-plan et `apres_identification` s'en chargera.
    if puuid_pour_api(etat, tx).is_some() {
        apres_identification(etat, tx);
    }
}

/// Id Data Dragon ("MonkeyKing") d'un champion.
fn id_dd(d: &DonneesJeu, champion: u16) -> Option<&str> {
    d.champions.get(&champion).map(|c| &*c.id)
}

fn champ_select(etat: &mut Etat, d: &DonneesJeu, cs: ChampSelect, tx: &Sender<Evenement>) {
    let role = cs.role.as_deref();
    affichage::modifier(|e| {
        let v = e.champ_select.get_or_insert_with(VueChampSelect::default);
        v.mon_champion = (cs.champion != 0).then(|| d.nom_champion(cs.champion).to_string());
        v.role = cs.role.as_deref().map(String::from);
        v.verrouille = cs.verrouille;
    });
    // 1. Mon champion.
    if cs.champion != 0 && cs.champion != etat.cs.champion_annonce {
        etat.cs.champion_annonce = cs.champion;
        let r = role.map(|r| format!(" ({r})")).unwrap_or_default();
        log!("\nChampion : {}{r}", d.nom_champion(cs.champion));
    }

    // 2. Runes + sorts : au verrouillage, une fois par champion.
    if cs.verrouille && cs.champion != 0 && etat.cs.runes_pour != cs.champion {
        etat.cs.runes_pour = cs.champion;
        runes(etat, d, cs.champion, role);
        objets(etat, d, &cs);
    }

    // 3. Compos (seulement quand un pick change).
    let allies: Vec<u16> = cs.champions_allies().collect();
    let cle = (allies.clone(), cs.ennemis.clone());
    if cle != etat.cs.compo_affichee {
        etat.cs.compo_affichee = cle;
        afficher_compos(d, &allies, &cs.ennemis, role, cs.verrouille);
    }

    // 4. Rangs des alliés : seulement si le client donne leur identité (hors classé).
    // Deux garde-fous : puuid absent / nom masqué (champselect.rs) ET file classée.
    // Par défaut ON BLOQUE : on ne cherche les alliés que si la file est CONNUE et non
    // classée. Si elle n'a pas pu être lue à l'entrée en champ select, un seul nouvel essai.
    if etat.file_gameflow.is_none() && !etat.cs.file_relue {
        etat.cs.file_relue = true;
        etat.file_gameflow = lire_file(etat);
    }
    let autorisee = matches!(etat.file_gameflow, Some(f) if !matches!(f, 420 | 440));
    if !etat.cs.rangs_demandes && autorisee {
        if let Some(api) = &etat.api {
            let connus: Vec<(String, u16, bool)> =
                cs.allies.iter().filter_map(|a| a.puuid.as_ref().map(|p| (p.to_string(), a.champion, a.moi))).collect();
            if connus.len() > 1 {
                etat.cs.rangs_demandes = true;
                crate::rangs::scouter_allies(api.clone(), etat.lcu.clone(), connus, etat.session_compte, tx.clone());
            }
        }
    }
    // 5. Matchup de lane et contre-picks (quand mon champion ou les ennemis changent).
    lane(etat, d, &cs);
    etat.cs.dernier = Some(cs);
}

fn afficher_compos(d: &DonneesJeu, allies: &[u16], ennemis: &[u16], role: Option<&str>, verrouille: bool) {
    let nom = |id: &str| d.champions.values().find(|c| &*c.id == id).map_or(id.to_string(), |c| c.nom.to_string());
    let a: Vec<&str> = allies.iter().filter_map(|&c| id_dd(d, c)).collect();
    let e: Vec<&str> = ennemis.iter().filter_map(|&c| id_dd(d, c)).collect();
    let analyse_a = compo::analyser(&a);
    let analyse_e = (!e.is_empty()).then(|| compo::analyser(&e));
    let plan = compo::comparer(&a, &e);
    log!("--- Compo alliée ({}/5) ---\n{}", a.len(), analyse_a.texte(&nom));
    if let Some(ae) = &analyse_e {
        log!("--- Compo ennemie ({}/5) ---\n{}", e.len(), ae.texte(&nom));
        for conseil in &plan {
            log!("  > {conseil}");
        }
    }
    // Picks conseillés tant que je n'ai pas verrouillé.
    let mut picks = Vec::new();
    if let (Some(r), false) = (role, verrouille) {
        picks = compo::suggestions(&a, &e, r, 5).into_iter().map(|x| (nom(x.id), x.raison)).collect();
        if !picks.is_empty() {
            let liste: Vec<String> = picks.iter().map(|(n, r)| format!("{n} ({r})")).collect();
            log!("  Picks qui complètent la compo : {}", liste.join(" · "));
        }
    }
    let noms = |ids: &[u16]| ids.iter().map(|&c| d.nom_champion(c).to_string()).collect::<Vec<_>>();
    let (na, ne) = (noms(allies), noms(ennemis));
    affichage::modifier(move |etat| {
        let v = etat.champ_select.get_or_insert_with(VueChampSelect::default);
        v.allies = na;
        v.ennemis = ne;
        v.analyse_allies = Some(analyse_a);
        v.analyse_ennemis = analyse_e;
        v.plan_de_jeu = plan;
        if !verrouille {
            v.picks_conseilles = picks;
        }
    });
}

fn nom_page(d: &DonneesJeu, page: &stats::PageRunes) -> String {
    let n = |id: u16| d.runes.get(&id).map_or(id.to_string(), |s| s.to_string());
    format!("{} + {} [{}]", n(page.perks[0]), n(page.secondaire), page.perks[1..6].iter().map(|&p| n(p)).collect::<Vec<_>>().join(", "))
}

fn runes(etat: &Etat, d: &DonneesJeu, champion: u16, role: Option<&str>) {
    let patch = stats::patch_court(&d.version);
    let Some(fichier) = stats::trouver_fichier(&patch) else {
        log!("Runes : pas encore de stats pour le patch {patch} (lance `stat-traque collecter 30`).");
        return;
    };
    let Some(r) = stats::recommander_depuis_fichier(&fichier, champion, role) else {
        log!("Runes : pas assez de parties pour {} sur ce patch.", d.nom_champion(champion));
        return;
    };
    let portee = r.role.as_deref().map_or("tous rôles".to_string(), String::from);
    log!(
        "Runes conseillées : {}  — {:.1} % de victoires sur {} parties (choisie dans {:.0} % des parties, {} parties analysées, {portee})",
        nom_page(d, &r.page), r.stats.winrate(), r.stats.parties, r.pick_rate, r.parties_champion
    );
    if r.plus_jouee.0 != r.page {
        log!("  Page la plus jouée : {}  — {:.1} % sur {} parties", nom_page(d, &r.plus_jouee.0), r.plus_jouee.1.winrate(), r.plus_jouee.1.parties);
    }
    let mut texte_sorts = None;
    let sorts = r.sorts.map(|((a, b), c)| {
        let n = |id: u16| d.sorts.get(&id).map_or(id.to_string(), |s| s.to_string());
        let t = format!("{} + {}  — {:.1} % sur {} parties", n(a), n(b), c.winrate(), c.parties);
        log!("Sorts conseillés : {t}");
        texte_sorts = Some(t);
        (a, b)
    });
    let vue = VueRunes {
        page: nom_page(d, &r.page),
        winrate: r.stats.winrate(),
        parties: r.stats.parties,
        pick_rate: r.pick_rate,
        parties_champion: r.parties_champion,
        sorts: texte_sorts,
        role: r.role.clone(),
    };
    affichage::modifier(|e| e.champ_select.get_or_insert_with(VueChampSelect::default).runes = Some(vue));

    // Application dans le client (pas en rejeu ; désactivable dans reglages.txt).
    let Some(lcu) = etat.lcu.clone().filter(|_| etat.mode != Mode::Rejeu) else { return };
    // Relus à chaque fois : un changement fait dans l'onglet Réglages compte tout de suite.
    let reglages = crate::stockage::Reglages::charger();
    let runes_auto = reglages.get("runes") != Some("non");
    let sorts_auto = reglages.get("sorts") != Some("non");
    let flash_sur_f = reglages.get("flash").is_some_and(|f| f.eq_ignore_ascii_case("f"));
    let page = r.page;
    let nom = format!("{} {}", d.nom_champion(champion), r.role.as_deref().unwrap_or(""));
    std::thread::spawn(move || {
        if runes_auto {
            match crate::lcu::actions::appliquer_runes(&lcu, nom.trim(), &page) {
                Ok(()) => log!("Page de runes appliquée."),
                Err(e) => log!("Runes non appliquées : {e}"),
            }
        }
        if let (true, Some(s)) = (sorts_auto, sorts) {
            match crate::lcu::actions::appliquer_sorts(&lcu, s, flash_sur_f) {
                Ok(()) => log!("Sorts appliqués."),
                Err(e) => log!("Sorts non appliqués : {e}"),
            }
        }
    });
}

/// Arena (mode CHERRY, files 1700/1710/1750) : l'API live et spectator ne donnent pas
/// les trios de façon fiable (tout le monde « ORDER » ou bleu, ou un mélange) : on
/// n'affiche alors aucune couleur d'équipe. `mode_live` = gameMode du port 2999.
fn noter_sans_equipes(etat: &mut Etat, mode_live: Option<&str>) {
    let arena_file = crate::rangs::file_partie()
        .or(etat.file_gameflow)
        .is_some_and(|f| crate::historique::famille(f) == crate::historique::Famille::Arena);
    etat.sans_equipes |= arena_file || mode_live == Some("CHERRY");
    // Publié dans les deux sens : une partie normale après une Arena repasse en équipes.
    let valeur = etat.sans_equipes;
    affichage::modifier(|e| e.sans_equipes = valeur);
}

fn afficher_rangs(d: &DonneesJeu, joueurs: &[InfoJoueur], sans_equipes: bool) {
    let mut joueurs = joueurs.to_vec();
    if !sans_equipes {
        joueurs.sort_by_key(|j| !j.bleu);
    }
    log!("--- Rangs ---");
    for j in &joueurs {
        let equipe = match (sans_equipes, j.bleu) {
            (true, _) => "     ",
            (false, true) => "BLEU ",
            (false, false) => "ROUGE",
        };
        let solo = j.solo.as_ref().map_or("non classé".into(), |r| {
            format!("{r} ({:.0} % sur {}){}", r.winrate(), r.victoires + r.defaites, if r.serie { " en série" } else { "" })
        });
        let flex = j.flex.as_ref().map_or(String::new(), |r| format!(" · flex {r}"));
        let meilleur = match (&j.meilleur, &j.solo) {
            (Some(m), Some(s)) if m.valeur() > s.valeur() => format!(" · meilleur vu {m}"),
            _ => String::new(),
        };
        let maitrise = j.maitrise.map_or(String::new(), |(n, p)| format!(" · maîtrise {n} ({} k pts)", p / 1000));
        let moi = if j.moi { " (toi)" } else { "" };
        let badges = if j.badges.is_empty() {
            String::new()
        } else {
            format!("  [{}]", j.badges.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
        };
        log!("{equipe} {:<13} {solo}{flex}{meilleur}{maitrise}  {}{moi}{badges}", d.nom_champion(j.champion), j.nom);
    }
}

fn afficher_partie(p: &InstantanePartie, sans_equipes: bool) {
    println!("\n--- Partie {} | {}:{:02} ---", p.mode, p.secondes / 60, p.secondes % 60);
    for j in &p.joueurs {
        let equipe = match (sans_equipes, j.bleu) {
            (true, _) => "",
            (false, true) => "BLEU",
            (false, false) => "ROUGE",
        };
        println!(
            "{equipe:<6} {:<14} niv {:>2}  {}/{}/{}  {:>3} cs  {}",
            j.champion, j.niveau, j.kills, j.morts, j.assists, j.cs, j.pseudo
        );
    }
}

// ---------- Stats étendues : matchups, contre-picks, objets, tier list ----------

/// Minimum de parties pour afficher un matchup ou proposer un contre-pick.
const MIN_PARTIES_MATCHUP: u32 = 10;

/// Au chargement de Data Dragon : télécharge le fichier de stats s'il manque, puis
/// calcule la tier list (lecture en flux du fichier, dans un thread à part : le
/// cerveau reste disponible).
fn preparer_stats_en_fond(d: &Arc<DonneesJeu>) {
    let d = d.clone();
    std::thread::spawn(move || {
        let patch = stats::patch_court(&d.version);
        if stats::trouver_fichier(&patch).is_none() {
            stats::mettre_a_jour(&patch);
        }
        let Some(t) = stats::trouver_fichier(&patch).and_then(|f| stats::tier_list_depuis_fichier(&f, None)) else {
            return;
        };
        // Fichier d'un ancien format (sans lignes C) ou trop peu de parties :
        // on ne publie pas une tier list vide.
        if t.lignes.is_empty() {
            log!("Tier list : pas de données exploitables pour le patch {patch} (ancien format ou trop peu de parties).");
            return;
        }
        let lignes: Vec<affichage::VueTier> = t
            .lignes
            .iter()
            .map(|l| affichage::VueTier {
                id: l.champion,
                champion: d.nom_champion(l.champion).to_string(),
                role: l.role,
                winrate: l.stats.winrate(),
                pick_rate: l.pick_rate,
                ban_rate: l.ban_rate,
                score: l.score,
                parties: l.stats.parties,
            })
            .collect();
        // RAM : ~80 octets par ligne (nom compris).
        log!("Tier list du patch {} : {} lignes ({} parties, ~{:.1} Ko)", t.patch, lignes.len(), t.parties, (lignes.len() * 80) as f32 / 1024.0);
        let vue = affichage::VueTierList { patch: t.patch, parties: t.parties, lignes };
        affichage::modifier(|e| e.tier_list = Some(std::sync::Arc::new(vue)));
    });
}

/// Stats du champ select : seulement les lignes de mon champion et des ennemis
/// visibles (lecture filtrée du fichier, jamais le fichier entier en RAM).
/// Relues seulement quand cette liste de champions change.
fn stats_du_champ_select<'a>(
    cache: &'a mut Option<(Vec<u16>, stats::Stats)>,
    d: &DonneesJeu,
    cs: &ChampSelect,
) -> Option<&'a stats::Stats> {
    // Champions utiles : les ennemis visibles + le mien (0 = pas encore choisi).
    let mut champions: Vec<u16> = cs.ennemis.to_vec();
    champions.push(cs.champion);
    champions.retain(|&c| c != 0);
    champions.sort_unstable();
    champions.dedup();
    // On ne relit le fichier que si un champion manque dans le cache (un champion
    // qui SORT de la liste, ex. un survol abandonné, ne coûte aucune relecture).
    let manque = |deja: &Vec<u16>| champions.iter().any(|c| !deja.contains(c));
    if cache.as_ref().is_none_or(|(deja, _)| manque(deja)) {
        let fichier = stats::trouver_fichier(&stats::patch_court(&d.version))?;
        let s = stats::Stats::charger_filtre(&fichier, &champions)?;
        log!("  (stats lues pour {} champions : ~{:.1} Ko en RAM)", champions.len(), s.memoire_estimee() as f32 / 1024.0);
        *cache = Some((champions, s));
    }
    cache.as_ref().map(|(_, s)| s)
}

fn vue_matchup(d: &DonneesJeu, l: &stats::LigneMatchup) -> affichage::VueMatchup {
    affichage::VueMatchup { champion: d.nom_champion(l.champion).to_string(), winrate: l.stats.winrate(), parties: l.stats.parties }
}

fn texte_matchups(liste: &[affichage::VueMatchup]) -> String {
    liste.iter().map(|m| format!("{} ({:.0} % sur {})", m.champion, m.winrate, m.parties)).collect::<Vec<_>>().join(" · ")
}

/// Adversaire de lane probable, mon matchup contre lui, et (avant de verrouiller)
/// les contre-picks. Uniquement des champions VISIBLES dans le champ select.
fn lane(etat: &mut Etat, d: &DonneesJeu, cs: &ChampSelect) {
    let Some(role) = cs.role.as_deref().and_then(|r| stats::ROLES.iter().copied().find(|x| *x == r)) else { return };
    let cle = (cs.champion, cs.ennemis.clone(), cs.verrouille);
    if etat.cs.lane_pour == cle {
        return;
    }
    etat.cs.lane_pour = cle;
    let annoncer_matchups = cs.champion != 0 && etat.cs.matchups_pour != cs.champion;
    let Some(s) = stats_du_champ_select(&mut etat.cs.stats_lues, d, cs) else { return };

    // Rôle habituel d'un champion sans stats : le premier rôle de son profil (compo).
    let par_defaut = |c: u16| id_dd(d, c).and_then(compo::profil).and_then(|p| p.roles.split(',').next());
    let adversaire = s.adversaire_probable(&cs.ennemis, role, &par_defaut);
    let matchup = adversaire
        .filter(|_| cs.champion != 0)
        .and_then(|a| s.matchup(cs.champion, Some(role), a))
        .filter(|c| c.parties >= MIN_PARTIES_MATCHUP);
    // Contre-picks : pas un champion déjà pris (allié ou ennemi).
    let pris: Vec<u16> = cs.champions_allies().chain(cs.ennemis.iter().copied()).collect();
    let contre: Vec<affichage::VueMatchup> = match (adversaire, cs.verrouille) {
        (Some(a), false) => s
            .contre_picks(a, role, MIN_PARTIES_MATCHUP, 20)
            .iter()
            .filter(|l| !pris.contains(&l.champion) || l.champion == cs.champion)
            .take(5)
            .map(|l| vue_matchup(d, l))
            .collect(),
        _ => Vec::new(),
    };
    let (meilleurs, pires) = if cs.champion != 0 {
        let (m, p) = s.matchups(cs.champion, role, MIN_PARTIES_MATCHUP, 3);
        (m.iter().map(|l| vue_matchup(d, l)).collect(), p.iter().map(|l| vue_matchup(d, l)).collect())
    } else {
        (Vec::new(), Vec::new())
    };

    // Journal.
    let nom_adv = adversaire.map(|a| d.nom_champion(a).to_string());
    let deja_annonce = adversaire.is_some_and(|a| etat.cs.matchup_annonce == (cs.champion, a));
    if let (Some(adv), false) = (&nom_adv, deja_annonce) {
        etat.cs.matchup_annonce = (cs.champion, adversaire.unwrap_or(0));
        match (cs.champion, matchup) {
            (0, _) => log!("Adversaire de lane probable : {adv}"),
            (moi, Some(c)) => log!(
                "Matchup : {} contre {adv} (adversaire probable) — {:.1} % de victoires sur {} parties",
                d.nom_champion(moi), c.winrate(), c.parties
            ),
            (moi, None) => log!("Matchup : {} contre {adv} (adversaire probable) — pas assez de parties", d.nom_champion(moi)),
        }
        if !contre.is_empty() {
            log!("  Contre-picks contre {adv} : {}", texte_matchups(&contre));
        }
    }
    if annoncer_matchups && !(meilleurs.is_empty() && pires.is_empty()) {
        etat.cs.matchups_pour = cs.champion;
        log!("  Meilleurs matchups de {} : {}", d.nom_champion(cs.champion), texte_matchups(&meilleurs));
        if !pires.is_empty() {
            log!("  Pires matchups : {}", texte_matchups(&pires));
        }
    }

    // Affichage.
    let vue_mu = matchup.zip(nom_adv.clone()).map(|(c, champion)| affichage::VueMatchup {
        champion,
        winrate: c.winrate(),
        parties: c.parties,
    });
    let verrouille = cs.verrouille;
    affichage::modifier(move |e| {
        let v = e.champ_select.get_or_insert_with(VueChampSelect::default);
        v.adversaire_lane = nom_adv;
        v.matchup = vue_mu;
        // Après le verrouillage, les contre-picks ne servent plus : on les vide.
        v.contre_picks = if verrouille { Vec::new() } else { contre };
        v.meilleurs_matchups = meilleurs;
        v.pires_matchups = pires;
    });
}

fn vue_objets(d: &DonneesJeu, c: &stats::ConseilObjets) -> affichage::VueObjets {
    let liste = |v: &[(u32, stats::Compteur)]| -> Vec<affichage::VueObjet> {
        v.iter()
            .map(|(id, n)| affichage::VueObjet {
                id: *id,
                nom: d.nom_objet(*id),
                frequence: c.frequence(n),
                winrate: n.winrate(),
                parties: n.parties,
            })
            .collect()
    };
    affichage::VueObjets {
        core: liste(&c.core),
        bottes: liste(&c.bottes),
        situationnels: liste(&c.situationnels),
        parties: c.parties,
        role: c.role.clone(),
        item_set_applique: false,
    }
}

/// Objets conseillés au verrouillage + item set « stat-traque <champion> » dans le
/// client (désactivable : `objets=non` dans reglages.txt).
fn objets(etat: &mut Etat, d: &DonneesJeu, cs: &ChampSelect) {
    let champion = cs.champion;
    let Some(s) = stats_du_champ_select(&mut etat.cs.stats_lues, d, cs) else { return };
    let Some(conseil) = s.conseil_objets(champion, cs.role.as_deref()) else {
        log!("Objets : pas assez de parties pour {} sur ce patch.", d.nom_champion(champion));
        return;
    };
    let vue = vue_objets(d, &conseil);
    let texte = |v: &[affichage::VueObjet]| {
        v.iter().map(|o| format!("{} ({:.0} % des parties, {:.1} % de victoires)", o.nom, o.frequence, o.winrate)).collect::<Vec<_>>().join(" · ")
    };
    log!("Objets core : {}", texte(&vue.core));
    if !vue.bottes.is_empty() {
        log!("  Bottes : {}", texte(&vue.bottes));
    }
    if !vue.situationnels.is_empty() {
        log!("  Situationnels : {}", vue.situationnels.iter().map(|o| o.nom.as_str()).collect::<Vec<_>>().join(", "));
    }
    affichage::modifier(|e| e.champ_select.get_or_insert_with(VueChampSelect::default).objets = Some(vue));

    // Item set dans le client (pas en rejeu, pas si désactivé, il faut mon summonerId).
    let Some(lcu) = etat.lcu.clone().filter(|_| etat.mode != Mode::Rejeu) else { return };
    if crate::stockage::Reglages::charger().get("objets") == Some("non") { // relu : modifiable dans la fenêtre
        return;
    }
    let Some(summoner_id) = etat.mon_summoner_id else {
        log!("Item set non appliqué : compte du client inconnu.");
        return;
    };
    let set = crate::lcu::actions::json_item_set(champion, d.nom_champion(champion), &conseil.blocs());
    std::thread::spawn(move || match crate::lcu::actions::appliquer_item_set(&lcu, summoner_id, set) {
        Ok(()) => {
            log!("Item set appliqué (boutique : « stat-traque … »).");
            affichage::modifier(|e| {
                if let Some(o) = e.champ_select.as_mut().and_then(|v| v.objets.as_mut()) {
                    o.item_set_applique = true;
                }
            });
        }
        Err(e) => log!("Item set non appliqué : {e}"),
    });
}
