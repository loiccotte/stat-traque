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
use std::sync::{Arc, Mutex};

use crate::serde_statique::Texte;
use crate::compo::AnalyseEquipe;
use crate::gameflow::Phase;
use crate::live::InstantanePartie;
use crate::rangs::InfoJoueur;


/// Lignes de journal gardées pour l'onglet « Journal ».
const MESSAGES_MAX: usize = 100;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueMatchup {
    /// L'autre champion : l'adversaire, ou le champion conseillé (contre-picks).
    pub champion: String,
    /// Winrate de mon côté (pour un contre-pick : celui du champion conseillé).
    pub winrate: f32,
    pub parties: u32,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueObjet {
    pub id: u32,
    pub nom: String,
    /// % des parties où l'objet est présent en fin de partie.
    pub frequence: f32,
    pub winrate: f32,
    pub parties: u32,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueTier {
    pub id: u16,
    pub champion: String,
    /// "top", "jungle", "middle", "bottom", "utility".
    #[serde(deserialize_with = "crate::serde_statique::texte")]
    pub role: Texte,
    pub winrate: f32,
    pub pick_rate: f32,
    pub ban_rate: f32,
    /// Winrate lissé (sert au tri).
    pub score: f32,
    pub parties: u32,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueTierList {
    pub patch: String,
    pub parties: u32,
    /// Triées par rôle puis par score décroissant.
    pub lignes: Vec<VueTier>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EtatAffichage {
    pub phase: Option<Phase>,
    pub client_connecte: bool,
    pub cle_api: bool,
    pub champ_select: Option<VueChampSelect>,
    pub rangs: Vec<InfoJoueur>,
    pub partie: Option<InstantanePartie>,
    /// Mode sans équipes fiables (Arena / CHERRY) : on n'affiche ni bleu ni rouge.
    /// L'API live et spectator n'y donnent pas les trios de façon fiable.
    #[serde(default)]
    pub sans_equipes: bool,
    pub messages: Vec<String>,
    /// Données du patch (noms des champions, objets...) : partagées, pas copiées
    /// (`Arc` = pointeur partagé, le cloner ne copie que le pointeur).
    /// Pas envoyées à la fenêtre : elle les relit dans le cache Data Dragon.
    #[serde(skip)]
    pub donnees: Option<Arc<crate::ddragon::DonneesJeu>>,
    // Les champs GROS ci-dessous sont dans des `Arc` : `lire()` ne copie qu'un
    // pointeur, et la fenêtre ne les reçoit que quand ils changent (`#[serde(skip)]` :
    // envoyés à part, voir ui/parent.rs). Pour les modifier : `e.historique = Arc::new(..)`.
    /// Tier list du patch (tous rôles), calculée une fois au démarrage.
    #[serde(skip)]
    pub tier_list: Option<Arc<VueTierList>>,
    // --- Historique et stats perso (feature/historique, voir historique.rs) ---
    /// Mes 20 dernières parties, plus récentes d'abord (champion = id : nom via Data Dragon).
    #[serde(skip)]
    pub historique: Arc<Vec<crate::historique::ResumePartie>>,
    /// Mes moyennes par champion / rôle / patch / file.
    #[serde(skip)]
    pub stats_perso: Option<Arc<crate::historique::StatsPerso>>,
    /// Rang actuel, meilleur rang, LP sur 7 jours, derniers points (graphique).
    #[serde(skip)]
    pub progression_lp: Option<Arc<crate::historique::ProgressionLp>>,
    /// Récap de la dernière partie, comparé à ma moyenne sur ce champion.
    pub recap: Option<crate::historique::Recap>,
    // --- Fiche joueur et partage (feature/fiche-joueur, voir fiche.rs) ---
    /// Dernière fiche demandée (moi ou un joueur de la partie). `Arc` : gros contenu,
    /// `lire()` ne copie qu'un pointeur. Envoyée à part (ui/protocole.rs, `Gros`).
    #[serde(skip)]
    pub fiche: Option<Arc<VueFiche>>,
    /// Une fiche est en cours de calcul (requêtes Riot en vol).
    pub fiche_en_cours: bool,
    /// Récap de la dernière partie, en texte Discord (bouton « Copier »).
    pub recap_partage: Option<String>,
    /// Joueurs dont on peut demander la fiche (partie en cours, via spectator).
    pub cibles_fiche: Vec<VueCibleFiche>,
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

/// Lit une PARTIE de l'état sans tout copier (ex : l'overlay ne veut que les rangs).
/// `f` s'exécute sous le verrou : elle doit être courte et ne pas appeler `modifier`.
pub fn avec<R>(f: impl FnOnce(&EtatAffichage) -> R) -> R {
    let verrou = ETAT.lock().unwrap();
    match verrou.as_ref() {
        Some(e) => f(e),
        None => f(&EtatAffichage::default()),
    }
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

/// Phase actuelle seulement (sans copier tout l'état) : l'interface s'en sert à
/// chaque changement pour ouvrir la fenêtre au champ select, gérer l'overlay...
pub fn phase() -> Option<Phase> {
    ETAT.lock().unwrap().as_ref().and_then(|e| e.phase)
}

// =====================================================================
// Fiche joueur (feature/fiche-joueur, voir fiche.rs). Types simples et
// sérialisables (String, nombres, Vec, Option) : envoyables à la fenêtre.
// =====================================================================

/// Un axe du radar de jeu.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueAxeRadar {
    /// "Combat", "Farm", "Vision", "Objectifs", "Phase de lane", "Survie", "Agressivité".
    pub nom: String,
    /// 0 à 100 ; 50 = moyenne des joueurs du même palier et du même rôle.
    /// None : pas assez de données (ex. parties sans métriques détaillées).
    pub score: Option<u8>,
}

/// MMR estimé (TOUJOURS une estimation : Riot ne publie pas le MMR).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueMmr {
    /// Ex : "~Émeraude II".
    pub rang_equivalent: String,
    /// Écart estimé en LP par rapport au rang affiché (+ = MMR au-dessus).
    pub ecart_lp: i32,
    /// "faible", "moyenne" ou "bonne".
    pub confiance: String,
    /// Les indices utilisés (« gains moyens +24 LP, pertes -16 LP »...).
    pub raisons: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueChampionFiche {
    pub champion: String,
    pub parties: u32,
    pub victoires: u32,
    pub winrate: f32,
    /// Part des parties analysées jouées sur ce champion, en %.
    pub part: f32,
    pub kda: f32,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueRoleFiche {
    /// "top", "jungle", "middle", "bottom", "utility".
    pub role: String,
    pub parties: u32,
    /// En %.
    pub part: f32,
}

/// La fiche d'un joueur (F41-F44).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueFiche {
    pub puuid: String,
    /// Riot ID ("Pseudo#TAG") ou "Moi".
    pub nom: String,
    pub moi: bool,
    /// Rang solo/duo affiché, ex. "Émeraude II 45 LP" (None : non classé).
    pub rang: Option<String>,
    /// Palier de la référence du radar, ex. "Émeraude".
    pub palier_reference: String,
    /// true : référence d'un palier voisin ou valeurs par défaut (à afficher).
    pub reference_approximative: bool,
    /// Les 7 axes, toujours dans le même ordre.
    pub radar: Vec<VueAxeRadar>,
    pub mmr: Option<VueMmr>,
    /// Champions les plus joués (5 au plus).
    pub champions: Vec<VueChampionFiche>,
    pub roles: Vec<VueRoleFiche>,
    /// Étiquettes de style : "Farmer", "Vision", "One-trick"...
    pub etiquettes: Vec<String>,
    /// 10 dernières parties, plus récente à gauche : "VVDVD...".
    pub forme: String,
    /// Série en cours : +3 = 3 victoires d'affilée, -2 = 2 défaites.
    pub serie: i32,
    /// Parties utilisées pour le radar.
    pub parties_analysees: u32,
    /// Requêtes Riot envoyées pour cette fiche (0 pour moi : base locale).
    pub requetes: u32,
    /// Problème rencontré (ex. clé expirée) ; la fiche peut être partielle.
    pub erreur: Option<String>,
    /// Texte Discord prêt à coller (bouton « Copier »).
    pub texte_partage: String,
}

/// Un joueur dont on peut demander la fiche (connu par spectator).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VueCibleFiche {
    pub puuid: String,
    /// Riot ID donné par spectator.
    pub nom: String,
    /// Champion joué dans la partie (id).
    pub champion: u16,
}
