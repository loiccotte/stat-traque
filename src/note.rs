//! La note d'un joueur (0 à 100) vue comme des étoiles. Fichier PUR : testable.
//!
//! Voir docs/DESIGN.md, section « La note ».

// Rempli et branché par les tâches suivantes (composants d'étoiles, overlay).
#![allow(dead_code)]

/// À partir de cette note (lettre S), les étoiles passent en or clair.
pub const SEUIL_S: u8 = 90;

/// Nombre d'étoiles allumées sur 5 : `ceil(note / 20)`, plafonné à 5 (0 pour une note nulle).
pub fn etoiles_allumees(note: u8) -> u8 {
    // Division entière arrondie vers le haut. En u16 : aucun dépassement possible.
    (note as u16).div_ceil(20).min(5) as u8
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
}
