//! Repères génériques de progression (CS par minute, niveau) pour la petite fenêtre en jeu.
//!
//! IMPORTANT : ce sont des repères INDICATIFS pour un rang moyen, pas une prédiction
//! ni une norme. Ils ne dépendent que du temps de jeu (donc de MA partie) : aucune
//! donnée adverse, aucun timer d'objectif ou de sort. Ce sont de simples tables
//! fixes : coût mémoire et CPU nul.

// Pas encore consommés : la petite fenêtre (autre tâche) les appellera.
#![allow(dead_code)]

/// Repère de CS par minute à un instant donné de la partie.
///
/// Courbe en 3 morceaux (interpolation linéaire) :
///  - 0 à 10 min : monte de 0 à 5,5 CS/min (les premières minutes sont creuses : vagues
///    pas encore arrivées) ;
///  - 10 à 25 min : monte de 5,5 à 6,5 CS/min ;
///  - au-delà de 25 min : plafonné à 6,5.
pub fn cs_min_attendu(secondes: u32) -> f32 {
    let minutes = secondes as f32 / 60.0;
    if minutes <= 10.0 {
        5.5 * minutes / 10.0
    } else if minutes <= 25.0 {
        5.5 + (minutes - 10.0) / 15.0 * 1.0
    } else {
        6.5
    }
}

/// Points de passage (minute, niveau) de la courbe générique de niveau.
const POINTS_NIVEAU: [(f32, f32); 5] = [(0.0, 1.0), (8.0, 6.0), (15.0, 9.0), (25.0, 13.0), (35.0, 18.0)];

/// Niveau généralement atteint à un instant donné (arrondi à l'inférieur), de 1 à 18.
///
/// On interpole entre les points de passage : niveau 6 vers 8 min, 9 vers 15 min,
/// 13 vers 25 min, 18 vers 35 min (plafond).
pub fn niveau_attendu(secondes: u32) -> u8 {
    let minutes = secondes as f32 / 60.0;
    for paire in POINTS_NIVEAU.windows(2) {
        let ((m0, n0), (m1, n1)) = (paire[0], paire[1]);
        if minutes < m1 {
            return (n0 + (minutes - m0) / (m1 - m0) * (n1 - n0)) as u8;
        }
    }
    18
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cs_min_valeurs_cles() {
        assert_eq!(cs_min_attendu(0), 0.0);
        assert!((cs_min_attendu(600) - 5.5).abs() < 0.01);
        assert!((cs_min_attendu(25 * 60) - 6.5).abs() < 0.01);
        assert!((cs_min_attendu(3 * 3600) - 6.5).abs() < 0.01); // plafonné
    }

    #[test]
    fn cs_min_croissant_et_borne() {
        let mut precedent = 0.0;
        for s in (0..=7200).step_by(10) {
            let v = cs_min_attendu(s);
            assert!(v >= precedent, "décroît à {s} s");
            assert!((0.0..=6.5).contains(&v));
            precedent = v;
        }
    }

    #[test]
    fn niveau_valeurs_cles() {
        assert_eq!(niveau_attendu(0), 1);
        assert_eq!(niveau_attendu(8 * 60), 6);
        assert_eq!(niveau_attendu(15 * 60), 9);
        assert_eq!(niveau_attendu(25 * 60), 13);
        assert_eq!(niveau_attendu(3 * 3600), 18); // plafonné
    }

    #[test]
    fn niveau_croissant_et_borne() {
        let mut precedent = 1;
        for s in (0..=7200).step_by(10) {
            let n = niveau_attendu(s);
            assert!(n >= precedent, "décroît à {s} s");
            assert!((1..=18).contains(&n));
            precedent = n;
        }
    }
}
