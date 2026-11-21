//! Liste des onglets de la fenêtre. Fichier PUR (pas de dessin) : testable.
//!
//! AJOUTER UN ONGLET (ex. « Matchups », « Tier list », « Récap ») :
//!  1. une variante dans `Onglet` + une ligne dans `Onglet::TOUS` (ordre d'affichage) ;
//!  2. son libellé dans `libelle()` ;
//!  3. une fonction `dessiner_xxx` dans `ecrans.rs` et une ligne dans le `match`
//!     de `ecrans::dessiner_onglet`. Le compilateur signale tout oubli.
//!
//! Un onglet peut se cacher tant qu'il n'a pas de données : voir `visible()`.

use crate::affichage::EtatAffichage;
use crate::gameflow::Phase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Onglet {
    ChampSelect,
    Partie,
    Historique,
    Journal,
    Reglages,
}

impl Onglet {
    /// Ordre d'affichage dans la barre d'onglets.
    pub const TOUS: [Onglet; 5] = [Onglet::ChampSelect, Onglet::Partie, Onglet::Historique, Onglet::Journal, Onglet::Reglages];

    pub fn libelle(self) -> &'static str {
        match self {
            Onglet::ChampSelect => "Champ select",
            Onglet::Partie => "Partie",
            Onglet::Historique => "Historique",
            Onglet::Journal => "Journal",
            Onglet::Reglages => "Réglages",
        }
    }

    /// Faut-il montrer l'onglet ? Les onglets de base sont toujours là (avec un
    /// message clair s'ils sont vides) ; un futur onglet optionnel renverra par
    /// exemple `!etat.matchups.is_empty()`.
    pub fn visible(self, _etat: &EtatAffichage) -> bool {
        true
    }
}

/// L'onglet le plus utile pour une phase : on y bascule quand la phase change
/// (le joueur reste libre de cliquer ailleurs ensuite).
pub fn onglet_conseille(phase: Option<Phase>) -> Option<Onglet> {
    match phase? {
        Phase::ChampSelect => Some(Onglet::ChampSelect),
        p if p.en_partie() => Some(Onglet::Partie),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onglet_selon_la_phase() {
        assert_eq!(onglet_conseille(Some(Phase::ChampSelect)), Some(Onglet::ChampSelect));
        assert_eq!(onglet_conseille(Some(Phase::InProgress)), Some(Onglet::Partie));
        assert_eq!(onglet_conseille(Some(Phase::Lobby)), None);
        assert_eq!(onglet_conseille(None), None);
        assert!(Onglet::TOUS.iter().all(|o| !o.libelle().is_empty() && o.visible(&EtatAffichage::default())));
    }
}
