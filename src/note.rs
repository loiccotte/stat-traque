//! La note d'un joueur (0 à 100) vue comme des étoiles. Fichier PUR : testable.
//!
//! Voir docs/DESIGN.md, section « La note ».
//!
//! ---------- Formule v1 (PROVISOIRE : à valider par le propriétaire avant d'afficher
//! de vraies valeurs) ----------
//! Entrées : les 7 scores d'axes de la fiche (`fiche::Radar::scores`, 0 à 100).
//! Chaque score est DÉJÀ comparé à la référence du même palier ET du même rôle
//! (voir fiche.rs : 50 = joueur moyen du palier, 70 = un écart-type au-dessus).
//!   note = moyenne des scores d'axes disponibles, pondérée par le rôle (`POIDS`),
//!          le total des poids étant normalisé sur les seuls axes PRÉSENTS,
//!          arrondie, bornée à 0..=100.
//! Donc une note de 50 = partie moyenne pour son rang et son rôle.
//! `None` (jamais de note inventée) si : rôle inconnu, référence approximative
//! (palier voisin ou valeurs par défaut), ou moins de 3 axes utilisables.
//! La victoire n'entre PAS dans la note : on note le jeu, pas le résultat.
//!
//! Limites connues :
//!  - les poids sont fixés à la main (intuition de jeu), pas mesurés ;
//!  - une partie de 18 minutes et une de 45 minutes sont comparées à la même référence ;
//!  - on ne compare pas à l'adversaire direct (un bon score contre un mauvais adversaire
//!    et un mauvais score contre un très bon sont notés pareil) ;
//!  - aucun axe n'est « décisif » : un seul axe très mauvais est dilué dans la moyenne.
//!
//! Évolutions prévues (NON implémentées) :
//!  1. références par tranche de durée de partie (moins de 25, 25 à 35, plus de 35 min) ;
//!  2. écart à l'adversaire direct à 15 minutes ;
//!  3. calibrage des poids par régression logistique (poids qui prédisent le mieux la
//!     victoire, sur les parties collectées).

/// À partir de cette note (lettre S), les étoiles passent en or clair.
pub const SEUIL_S: u8 = 90;

/// Nombre d'axes du radar (même ordre que `fiche::AXES`).
const NB_AXES: usize = 7;
/// Nombre minimal d'axes utilisables pour oser donner une note.
const MIN_AXES: usize = 3;

/// Poids de chaque axe, par rôle. Ordre des colonnes = `fiche::AXES` :
/// Combat, Farm, Vision, Objectifs, Phase de lane, Survie, Agressivité.
/// Un poids de 0 = l'axe ne compte pas pour ce rôle (le farm d'un support).
/// Valeurs choisies à la main, relatives entre elles (seule la proportion compte).
const POIDS: [(&str, [u8; NB_AXES]); 5] = [
    //              Combat Farm Vision Obj. Lane Survie Agress.
    ("top", [3, 2, 1, 2, 3, 2, 1]),     // lane et combat d'abord
    ("jungle", [3, 1, 2, 4, 1, 2, 2]),  // objectifs et vision : le rôle de la jungle
    ("middle", [3, 3, 1, 1, 3, 2, 2]),  // combat, farm et lane
    ("bottom", [3, 4, 1, 1, 2, 2, 1]),  // le farm fait le carry
    ("utility", [2, 0, 5, 2, 1, 2, 1]), // la vision avant tout, pas de farm
];

/// Nombre d'étoiles allumées sur 5 : `ceil(note / 20)`, plafonné à 5 (0 pour une note nulle).
pub fn etoiles_allumees(note: u8) -> u8 {
    // Division entière arrondie vers le haut. En u16 : aucun dépassement possible.
    (note as u16).div_ceil(20).min(5) as u8
}

// Pas encore affichée par un écran (seuls les tests l'appellent) : à brancher ou supprimer.
#[allow(dead_code)]
/// Lettre de la note : D (< 45), C (< 60), B (< 75), A (< 90), S (90 et plus).
pub fn lettre(note: u8) -> char {
    match note {
        0..=44 => 'D',
        45..=59 => 'C',
        60..=74 => 'B',
        75..=89 => 'A',
        _ => 'S',
    }
}

/// Note 0-100 d'une partie (ou d'un joueur) d'après ses 7 scores d'axes.
///  - `scores` : `fiche::Radar::scores` (None = axe sans donnée) ;
///  - `role` : "top", "jungle", "middle", "bottom" ou "utility" (rôle Riot) ;
///  - `approximatif` : `fiche::Radar::approximatif` (référence d'un palier voisin ou
///    par défaut : on préfère ne rien afficher plutôt qu'une note trompeuse).
///
/// `None` si le rôle est inconnu, si la référence est approximative ou si moins de
/// 3 axes (de poids non nul) sont disponibles.
pub fn note_partie(scores: &[Option<u8>; NB_AXES], role: Option<&str>, approximatif: bool) -> Option<u8> {
    if approximatif {
        return None;
    }
    let role = role?;
    let poids = POIDS.iter().find(|(r, _)| *r == role)?.1;
    // On additionne seulement les axes présents ; le total des poids suit.
    let (mut somme, mut total, mut axes) = (0u32, 0u32, 0usize);
    for (score, w) in scores.iter().zip(poids) {
        if let (Some(s), true) = (score, w > 0) {
            somme += *s.min(&100) as u32 * w as u32;
            total += w as u32;
            axes += 1;
        }
    }
    if axes < MIN_AXES {
        return None;
    }
    // Moyenne arrondie au plus proche, en entiers : (2·somme + total) / (2·total).
    let note = (2 * somme + total) / (2 * total);
    Some(note.min(100) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etoiles_selon_la_note() {
        for (note, attendu) in [(0, 0), (1, 1), (20, 1), (21, 2), (89, 5), (90, 5), (100, 5)] {
            assert_eq!(etoiles_allumees(note), attendu, "note {note}");
        }
    }

    #[test]
    fn plafonne_a_cinq() {
        assert_eq!(etoiles_allumees(255), 5);
    }

    #[test]
    fn lettre_aux_seuils() {
        for (note, attendu) in [
            (0, 'D'), (44, 'D'), (45, 'C'), (59, 'C'), (60, 'B'), (74, 'B'),
            (75, 'A'), (89, 'A'), (90, 'S'), (100, 'S'), (255, 'S'),
        ] {
            assert_eq!(lettre(note), attendu, "note {note}");
        }
        assert_eq!(lettre(SEUIL_S), 'S');
    }

    #[test]
    fn la_table_de_poids_suit_le_radar() {
        assert_eq!(NB_AXES, crate::fiche::AXES.len());
        for (role, p) in POIDS {
            assert!(p.iter().any(|w| *w > 0), "{role}");
        }
    }

    #[test]
    fn joueur_moyen_note_cinquante() {
        for (role, _) in POIDS {
            assert_eq!(note_partie(&[Some(50); 7], Some(role), false), Some(50), "{role}");
        }
    }

    #[test]
    fn bornes_zero_et_cent() {
        assert_eq!(note_partie(&[Some(0); 7], Some("top"), false), Some(0));
        assert_eq!(note_partie(&[Some(100); 7], Some("top"), false), Some(100));
        // Un score hors échelle ne fait pas dépasser 100.
        assert_eq!(note_partie(&[Some(255); 7], Some("top"), false), Some(100));
    }

    #[test]
    fn monotone_quand_un_axe_monte() {
        let mut precedente = 0;
        for s in (0..=100).step_by(5) {
            let mut scores = [Some(40); 7];
            scores[1] = Some(s); // le farm monte
            let n = note_partie(&scores, Some("bottom"), false).unwrap();
            assert!(n >= precedente, "farm {s} : {n} < {precedente}");
            precedente = n;
        }
    }

    #[test]
    fn le_role_change_le_poids() {
        // Excellent en vision, médiocre ailleurs : le support en profite plus que le mid.
        let mut scores = [Some(40); 7];
        scores[2] = Some(95);
        let support = note_partie(&scores, Some("utility"), false).unwrap();
        let mid = note_partie(&scores, Some("middle"), false).unwrap();
        assert!(support > mid, "{support} <= {mid}");
        // Le farm ne compte pas pour un support.
        let mut sans_farm = scores;
        sans_farm[1] = Some(0);
        assert_eq!(note_partie(&sans_farm, Some("utility"), false), Some(support));
    }

    #[test]
    fn axes_manquants_normalises() {
        // 4 axes à 80, les autres absents : la note reste 80 (pas tirée vers 0).
        let scores = [Some(80), Some(80), None, Some(80), None, Some(80), None];
        assert_eq!(note_partie(&scores, Some("middle"), false), Some(80));
    }

    #[test]
    fn none_si_donnees_insuffisantes() {
        // Moins de 3 axes.
        let deux = [Some(70), Some(70), None, None, None, None, None];
        assert_eq!(note_partie(&deux, Some("middle"), false), None);
        assert_eq!(note_partie(&[None; 7], Some("middle"), false), None);
        // Support : le farm (poids 0) ne compte pas dans les 3 axes minimaux.
        let deux_axes_et_farm_sans_poids = [Some(70), Some(70), None, None, None, None, Some(70)];
        assert_eq!(note_partie(&deux_axes_et_farm_sans_poids, Some("utility"), false), None);
        // Trois axes utilisables : une note.
        let trois = [Some(70), Some(70), None, None, None, None, Some(70)];
        assert_eq!(note_partie(&trois, Some("middle"), false), Some(70));
        // Rôle inconnu, rôle absent, référence approximative.
        assert_eq!(note_partie(&[Some(70); 7], Some("arena"), false), None);
        assert_eq!(note_partie(&[Some(70); 7], None, false), None);
        assert_eq!(note_partie(&[Some(70); 7], Some("middle"), true), None);
    }
}
