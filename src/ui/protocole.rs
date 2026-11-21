//! Ce que le processus principal envoie au processus de la fenêtre : une ligne
//! JSON par message, sur l'entrée standard de l'enfant. Fichier PUR (testé).
//!
//! Pourquoi un 2e processus ? Le pilote OpenGL, une fois chargé, reste en mémoire
//! jusqu'à la fin du processus (10 à 30 Mo). La fenêtre vit donc dans un processus
//! à part, qui MEURT à la fermeture : la RAM revient vraiment au système.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::affichage::{EtatAffichage, VueFiche, VueTierList};
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
}
