//! Ce que le processus principal envoie au processus de la fenêtre : une ligne
//! JSON par message, sur l'entrée standard de l'enfant. Fichier PUR (testé).
//!
//! Et dans l'autre sens (canal RETOUR) : l'enfant écrit ses DEMANDES (`Demande`),
//! une ligne JSON chacune, sur sa sortie standard ; le parent les lit dans un
//! thread et les traduit en événements pour le cerveau (`traduire`). L'enfant
//! n'est PAS digne de confiance : tout est revérifié ici et dans le cerveau.
//!
//! Pourquoi un 2e processus ? Le pilote OpenGL, une fois chargé, reste en mémoire
//! jusqu'à la fin du processus (10 à 30 Mo). La fenêtre vit donc dans un processus
//! à part, qui MEURT à la fermeture : la RAM revient vraiment au système.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::affichage::{EtatAffichage, VueCibleFiche, VueFiche, VueTierList};
use crate::evenement::Evenement;
use crate::historique::{ProgressionLp, ResumePartie, StatsPerso};

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    /// L'état, SANS les gros champs (envoyés à part, seulement quand ils changent).
    Etat(Box<EtatAffichage>),
    Gros(Gros),
    /// Ramener la fenêtre au premier plan (clic sur l'icône alors qu'elle est ouverte).
    PremierPlan,
    /// Démonstration (`essai-fenetre`) : utiliser les noms de champions de la démo.
    Demo,
}

/// Les champs gros de l'état (des `Arc` : les copier ne coûte qu'un pointeur).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Gros {
    pub tier_list: Option<Arc<VueTierList>>,
    pub historique: Arc<Vec<ResumePartie>>,
    pub stats_perso: Option<Arc<StatsPerso>>,
    pub progression_lp: Option<Arc<ProgressionLp>>,
    /// Fiche joueur (fiche.rs).
    pub fiche: Option<Arc<VueFiche>>,
}

impl Gros {
    fn depuis(e: &EtatAffichage) -> Self {
        Gros {
            tier_list: e.tier_list.clone(),
            historique: e.historique.clone(),
            stats_perso: e.stats_perso.clone(),
            progression_lp: e.progression_lp.clone(),
            fiche: e.fiche.clone(),
        }
    }

    /// Même contenu ? On compare les POINTEURS (`Arc::ptr_eq`) : instantané, et
    /// le cerveau remplace l'`Arc` à chaque nouvelle valeur.
    fn identique(&self, autre: &Gros) -> bool {
        fn meme<T>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
        }
        meme(&self.tier_list, &autre.tier_list)
            && Arc::ptr_eq(&self.historique, &autre.historique)
            && meme(&self.stats_perso, &autre.stats_perso)
            && meme(&self.progression_lp, &autre.progression_lp)
            && meme(&self.fiche, &autre.fiche)
    }
}

/// Côté processus principal : fabrique les lignes à envoyer pour un nouvel état.
#[derive(Default)]
pub struct Expediteur {
    gros_envoyes: Option<Gros>,
}

impl Expediteur {
    pub fn lignes(&mut self, etat: EtatAffichage) -> Vec<String> {
        let mut lignes = Vec::new();
        let gros = Gros::depuis(&etat);
        if !self.gros_envoyes.as_ref().is_some_and(|g| g.identique(&gros)) {
            lignes.push(ligne(&Message::Gros(gros.clone())));
            self.gros_envoyes = Some(gros);
        }
        lignes.push(ligne(&Message::Etat(Box::new(etat))));
        lignes
    }
}

/// Un message -> une ligne JSON (sans retour à la ligne dedans : JSON l'échappe).
pub fn ligne(m: &Message) -> String {
    serde_json::to_string(m).unwrap_or_default()
}

/// Côté fenêtre : applique un message reçu à l'état local.
/// Renvoie les messages qui ne changent pas l'état (`PremierPlan`, `Demo`) ;
/// `None` sinon. Une ligne illisible est ignorée.
pub fn appliquer(etat: &mut EtatAffichage, texte: &str) -> Option<Message> {
    appliquer_message(etat, lire_message(texte)?)
}

/// Une ligne -> un message (None si illisible). Se fait SANS verrou (c'est le plus long).
pub fn lire_message(texte: &str) -> Option<Message> {
    serde_json::from_str(texte).ok()
}

/// Version de stat-traque, en 1re ligne (texte brut, pas du JSON : lisible même si
/// le format des messages a changé entre deux versions).
pub fn ligne_version() -> String {
    format!("version={}", env!("CARGO_PKG_VERSION"))
}

/// La 1re ligne reçue vient-elle d'un stat-traque de la MÊME version que nous ?
/// (Après une mise à jour automatique, l'exe sur le disque est plus récent que
/// le processus de l'icône qui tourne encore : les formats peuvent différer.)
pub fn meme_version(premiere_ligne: &str) -> bool {
    premiere_ligne.trim() == ligne_version()
}

/// Applique un message déjà lu (rapide : à faire sous verrou).
pub fn appliquer_message(etat: &mut EtatAffichage, message: Message) -> Option<Message> {
    match message {
        Message::Etat(nouveau) => {
            let mut nouveau = *nouveau;
            // Garder ce qui n'est pas dans le message : données du patch et gros champs.
            nouveau.donnees = etat.donnees.take();
            nouveau.tier_list = etat.tier_list.take();
            nouveau.historique = std::mem::take(&mut etat.historique);
            nouveau.stats_perso = etat.stats_perso.take();
            nouveau.progression_lp = etat.progression_lp.take();
            nouveau.fiche = etat.fiche.take();
            *etat = nouveau;
            None
        }
        Message::Gros(g) => {
            etat.tier_list = g.tier_list;
            etat.historique = g.historique;
            etat.stats_perso = g.stats_perso;
            etat.progression_lp = g.progression_lp;
            etat.fiche = g.fiche;
            None
        }
        autre => Some(autre),
    }
}

// =====================================================================
// Canal retour : demandes de la fenêtre (enfant -> parent)
// =====================================================================

/// Ce que la fenêtre peut DEMANDER au processus principal. Liste fermée : pas de
/// commande à exécuter, pas de chemin de fichier, seulement ces deux demandes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Demande {
    /// Fiche d'un joueur de la partie en cours (puuid donné par `cibles_fiche`).
    Fiche { puuid: String },
    /// Ma propre fiche (base locale, 0 requête Riot).
    MaFiche,
}

/// Longueur maximale d'une ligne de demande lue par le parent (octets). Une vraie
/// demande fait ~100 octets ; au-delà, la ligne est jetée sans être gardée en RAM.
pub const LIGNE_DEMANDE_MAX: usize = 512;

/// Un puuid Riot : 78 caractères en pratique, lettres, chiffres, `-` et `_`.
/// On accepte 1 à 100 caractères de ce jeu-là, rien d'autre.
fn puuid_valide(p: &str) -> bool {
    (1..=100).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Côté fenêtre : une demande -> une ligne JSON (sans retour à la ligne).
pub fn ligne_demande(d: &Demande) -> String {
    serde_json::to_string(d).unwrap_or_default()
}

/// Côté parent : une ligne -> une demande. None si la ligne est illisible, trop
/// longue, ou si le puuid a une forme impossible (ligne ignorée).
pub fn lire_demande(texte: &str) -> Option<Demande> {
    let texte = texte.trim();
    if texte.len() > LIGNE_DEMANDE_MAX {
        return None;
    }
    match serde_json::from_str::<Demande>(texte).ok()? {
        Demande::Fiche { puuid } if !puuid_valide(&puuid) => None,
        d => Some(d),
    }
}

/// Côté parent : demande -> événement pour le cerveau. Re-vérification : un puuid
/// qui n'est PAS dans `cibles` (joueurs de ma partie connus par spectator, liste
/// tenue par le processus principal, jamais par l'enfant) est ignoré. Le cerveau
/// revérifie encore derrière (`fiche::nom_autorise`).
pub fn traduire(d: Demande, cibles: &[VueCibleFiche]) -> Option<Evenement> {
    match d {
        Demande::MaFiche => Some(Evenement::DemandeMaFiche),
        Demande::Fiche { puuid } => {
            cibles.iter().any(|c| c.puuid == puuid).then(|| Evenement::DemandeFiche { puuid: puuid.into() })
        }
    }
}

/// Lit UNE ligne de `entree` sans jamais garder plus de `max` octets en mémoire :
/// une ligne plus longue est lue jusqu'au bout puis jetée (`Some(None)`, de même
/// qu'une ligne qui n'est pas de l'UTF-8). Renvoie None à la fin du flux.
pub fn ligne_bornee(entree: &mut impl std::io::BufRead, max: usize) -> Option<Option<String>> {
    let mut tampon = Vec::new();
    let mut trop_longue = false;
    loop {
        let morceau = match entree.fill_buf() {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        if morceau.is_empty() {
            // Fin du flux : une dernière ligne sans retour à la ligne compte quand même.
            return (!tampon.is_empty() || trop_longue).then(|| fin_de_ligne(tampon, trop_longue));
        }
        // Jusqu'où lire dans ce morceau : le retour à la ligne, ou tout le morceau.
        let (a_prendre, fin) = match morceau.iter().position(|&b| b == b'\n') {
            Some(i) => (i, true),
            None => (morceau.len(), false),
        };
        if !trop_longue {
            if tampon.len() + a_prendre > max {
                trop_longue = true;
                tampon = Vec::new(); // on libère tout de suite
            } else {
                tampon.extend_from_slice(&morceau[..a_prendre]);
            }
        }
        entree.consume(a_prendre + usize::from(fin));
        if fin {
            return Some(fin_de_ligne(tampon, trop_longue));
        }
    }
}

fn fin_de_ligne(tampon: Vec<u8>, trop_longue: bool) -> Option<String> {
    if trop_longue {
        None
    } else {
        String::from_utf8(tampon).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gameflow::Phase;
    use crate::ui::demo;

    #[test]
    fn aller_retour_complet() {
        let depart = demo::etat(Phase::InProgress);
        let mut exp = Expediteur::default();
        let lignes = exp.lignes(depart.clone());
        assert_eq!(lignes.len(), 2, "gros + état");
        assert!(lignes.iter().all(|l| !l.contains('\n')));

        let mut recu = EtatAffichage::default();
        for l in &lignes {
            assert!(appliquer(&mut recu, l).is_none());
        }
        assert_eq!(recu.phase, Some(Phase::InProgress));
        assert_eq!(recu.rangs, depart.rangs);
        assert_eq!(recu.partie, depart.partie);
        assert_eq!(recu.historique.len(), depart.historique.len());
        assert_eq!(recu.tier_list.as_ref().unwrap().lignes, depart.tier_list.as_ref().unwrap().lignes);
        assert_eq!(recu.recap, depart.recap);
        let cs = recu.champ_select.as_ref().unwrap();
        assert_eq!(cs.analyse_allies.as_ref().unwrap().roles, depart.champ_select.as_ref().unwrap().analyse_allies.as_ref().unwrap().roles);
        assert!(recu.donnees.is_none(), "les données du patch ne voyagent pas");

        // Même état : seulement l'état, pas les gros champs (même Arc).
        assert_eq!(exp.lignes(depart.clone()).len(), 1);
        // Un nouvel historique (nouvel Arc) : renvoyé.
        let mut change = depart;
        change.historique = Arc::new(Vec::new());
        assert_eq!(exp.lignes(change.clone()).len(), 2);
        // Un message d'état seul ne fait pas perdre les gros champs déjà reçus.
        assert!(appliquer(&mut recu, &ligne(&Message::Etat(Box::new(change)))).is_none());
        assert!(recu.tier_list.is_some());
    }

    #[test]
    fn controle_de_version() {
        assert!(meme_version(&ligne_version()));
        assert!(meme_version(&format!("{}\r", ligne_version())));
        assert!(!meme_version("version=0.0.1"));
        assert!(!meme_version("{\"Etat\":{}}"));
    }

    #[test]
    fn messages_simples_et_lignes_abimees() {
        let mut e = EtatAffichage::default();
        assert!(matches!(appliquer(&mut e, &ligne(&Message::PremierPlan)), Some(Message::PremierPlan)));
        assert!(appliquer(&mut e, "pas du json").is_none());
    }

    #[test]
    fn demandes_aller_retour() {
        for d in [Demande::MaFiche, Demande::Fiche { puuid: "abc-DEF_123".into() }] {
            let l = ligne_demande(&d);
            assert!(!l.contains('\n'));
            assert_eq!(lire_demande(&l), Some(d.clone()));
            assert_eq!(lire_demande(&format!("{l}\r")), Some(d), "fin de ligne Windows");
        }
    }

    #[test]
    fn demandes_invalides_ignorees() {
        for l in [
            "",
            "pas du json",
            r#"{"Fiche":{}}"#,
            r#"{"Executer":{"commande":"cmd.exe"}}"#,
            r#"{"Fiche":{"puuid":""}}"#,
            r#"{"Fiche":{"puuid":"../../secret"}}"#,
            r#"{"Fiche":{"puuid":"a b"}}"#,
            r#"{"Fiche":{"puuid":42}}"#,
            r#""MaFiche" en trop"#,
        ] {
            assert_eq!(lire_demande(l), None, "{l}");
        }
        let longue = format!(r#"{{"Fiche":{{"puuid":"{}"}}}}"#, "a".repeat(600));
        assert_eq!(lire_demande(&longue), None);
    }

    #[test]
    fn traduction_verifie_les_cibles() {
        let cibles = vec![VueCibleFiche { puuid: "p1".into(), nom: "Ami#EUW".into(), champion: 103 }];
        assert!(matches!(traduire(Demande::MaFiche, &[]), Some(Evenement::DemandeMaFiche)));
        match traduire(Demande::Fiche { puuid: "p1".into() }, &cibles) {
            Some(Evenement::DemandeFiche { puuid }) => assert_eq!(&*puuid, "p1"),
            autre => panic!("{autre:?}"),
        }
        assert!(traduire(Demande::Fiche { puuid: "inconnu".into() }, &cibles).is_none());
        assert!(traduire(Demande::Fiche { puuid: "p1".into() }, &[]).is_none());
    }

    #[test]
    fn lignes_bornees() {
        let texte = format!("{}\n{}\ncourte\r\nfin", "x".repeat(10), "y".repeat(50));
        // Petit tampon : les lignes arrivent en plusieurs morceaux.
        let mut r = std::io::BufReader::with_capacity(8, texte.as_bytes());
        assert_eq!(ligne_bornee(&mut r, 20), Some(Some("x".repeat(10))));
        assert_eq!(ligne_bornee(&mut r, 20), Some(None), "trop longue : jetée");
        assert_eq!(ligne_bornee(&mut r, 20), Some(Some("courte\r".into())));
        assert_eq!(ligne_bornee(&mut r, 20), Some(Some("fin".into())));
        assert_eq!(ligne_bornee(&mut r, 20), None);
        let mut abime = std::io::BufReader::new(&[0xff, 0xfe, b'\n'][..]);
        assert_eq!(ligne_bornee(&mut abime, 20), Some(None), "UTF-8 invalide");
    }
}
