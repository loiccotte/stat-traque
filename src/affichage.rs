//! État à afficher : ce que la fenêtre (et l'overlay) montrent.
//!
//! CONTRAT entre le cerveau et l'interface :
//!  - le cerveau REMPLIT cet état avec `modifier(|e| ...)` ;
//!  - l'interface le LIT avec `lire()` (copie) et se redessine quand `version()` change.
//!
//! L'interface ne parle jamais directement au client LoL ni à l'API Riot.
//! Les autres features ajoutent ici leurs champs (historique, matchups...).

// Les champs ne sont lus que par l'interface (feature/interface), pas encore fusionnée.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::compo::AnalyseEquipe;
use crate::gameflow::Phase;
use crate::live::InstantanePartie;
use crate::rangs::InfoJoueur;


/// Lignes de journal gardées pour l'onglet « Journal ».
const MESSAGES_MAX: usize = 100;

#[derive(Debug, Clone, Default)]
pub struct VueRunes {
    /// Ex : "Électrocution + Inspiration [Impact soudain, ...]"
    pub page: String,
    pub winrate: f32,
    pub parties: u32,
    pub pick_rate: f32,
    pub parties_champion: u32,
    /// Ex : "Flash + Embrasement — 52.1 % sur 3 400 parties"
    pub sorts: Option<String>,
    /// Rôle des stats, None = tous rôles confondus.
    pub role: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct VueChampSelect {
    pub mon_champion: Option<String>,
    pub role: Option<String>,
    pub verrouille: bool,
    /// Noms affichés des champions choisis.
    pub allies: Vec<String>,
    pub ennemis: Vec<String>,
    pub analyse_allies: Option<AnalyseEquipe>,
    pub analyse_ennemis: Option<AnalyseEquipe>,
    pub plan_de_jeu: Vec<String>,
    /// (nom du champion, raison)
    pub picks_conseilles: Vec<(String, String)>,
    pub runes: Option<VueRunes>,
    // --- Stats étendues (feature/stats-etendues) ---
    /// Adversaire PROBABLE de ma lane (nom) : déduit des rôles habituels des
    /// ennemis visibles, le client ne donne pas leurs rôles.
    pub adversaire_lane: Option<String>,
    /// Mon champion contre cet adversaire (None = pas assez de parties).
    pub matchup: Option<VueMatchup>,
    /// Avant le verrouillage : champions de mon rôle qui gagnent le plus contre lui.
    pub contre_picks: Vec<VueMatchup>,
    /// Meilleurs / pires adversaires de mon champion à mon rôle.
    pub meilleurs_matchups: Vec<VueMatchup>,
    pub pires_matchups: Vec<VueMatchup>,
    /// Objets conseillés (rempli au verrouillage).
    pub objets: Option<VueObjets>,
}

/// Un duel de lane, vu de MON côté.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VueMatchup {
    /// L'autre champion : l'adversaire, ou le champion conseillé (contre-picks).
    pub champion: String,
    /// Winrate de mon côté (pour un contre-pick : celui du champion conseillé).
    pub winrate: f32,
    pub parties: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VueObjet {
    pub id: u32,
    pub nom: String,
    /// % des parties où l'objet est présent en fin de partie.
    pub frequence: f32,
    pub winrate: f32,
    pub parties: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VueObjets {
    /// Les 3 légendaires les plus construits.
    pub core: Vec<VueObjet>,
    pub bottes: Vec<VueObjet>,
    pub situationnels: Vec<VueObjet>,
    /// Parties analysées pour ce champion.
    pub parties: u32,
    /// Rôle des stats, None = tous rôles confondus.
    pub role: Option<String>,
    /// true quand l'item set « stat-traque <champion> » est enregistré dans le client.
    pub item_set_applique: bool,
}

/// Une ligne de la tier list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VueTier {
    pub id: u16,
    pub champion: String,
    /// "top", "jungle", "middle", "bottom", "utility".
    pub role: &'static str,
    pub winrate: f32,
    pub pick_rate: f32,
    pub ban_rate: f32,
    /// Winrate lissé (sert au tri).
    pub score: f32,
    pub parties: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VueTierList {
    pub patch: String,
    pub parties: u32,
    /// Triées par rôle puis par score décroissant.
    pub lignes: Vec<VueTier>,
}

#[derive(Debug, Clone, Default)]
pub struct EtatAffichage {
    pub phase: Option<Phase>,
    pub client_connecte: bool,
    pub cle_api: bool,
    pub champ_select: Option<VueChampSelect>,
    pub rangs: Vec<InfoJoueur>,
    pub partie: Option<InstantanePartie>,
    pub messages: Vec<String>,
    /// Tier list du patch (tous rôles), calculée une fois au démarrage.
    pub tier_list: Option<VueTierList>,
}

static ETAT: Mutex<Option<EtatAffichage>> = Mutex::new(None);
static VERSION: AtomicU64 = AtomicU64::new(0);
/// Fonction appelée après chaque modification (l'interface s'y abonne pour se redessiner).
static REVEIL: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

/// Modifie l'état puis prévient l'interface.
pub fn modifier(f: impl FnOnce(&mut EtatAffichage)) {
    f(ETAT.lock().unwrap().get_or_insert_with(EtatAffichage::default));
    VERSION.fetch_add(1, Ordering::Release);
    if let Some(r) = REVEIL.lock().unwrap().as_ref() {
        r();
    }
}

/// Copie de l'état actuel.
pub fn lire() -> EtatAffichage {
    ETAT.lock().unwrap().clone().unwrap_or_default()
}

/// Change à chaque modification : l'interface ne copie l'état que si ça a changé.
pub fn version() -> u64 {
    VERSION.load(Ordering::Acquire)
}

/// L'interface enregistre ici sa fonction « redessine-toi ».
pub fn quand_ca_change(f: impl Fn() + Send + 'static) {
    *REVEIL.lock().unwrap() = Some(Box::new(f));
}

/// Ajoute une ligne de journal (appelé par `journal::ecrire`).
pub fn message(texte: String) {
    modifier(|e| {
        e.messages.push(texte);
        if e.messages.len() > MESSAGES_MAX {
            e.messages.remove(0);
        }
    });
}
