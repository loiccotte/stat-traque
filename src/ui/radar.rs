//! Géométrie du radar de la fiche joueur (7 axes). Fichier PUR : que des calculs
//! sur des nombres (pas d'egui), donc testable. Le dessin est dans ui/fiche.rs.
//!
//! Repère de l'écran : x vers la droite, y vers le BAS. L'axe n° 0 est en haut,
//! les suivants tournent dans le sens des aiguilles d'une montre.

use std::f32::consts::{FRAC_PI_2, TAU};

/// Cercles de graduation (en score 0 à 100) : 25, 50 (= la moyenne), 75.
pub const GRADUATIONS: [u8; 3] = [25, 50, 75];

/// Angle (radians) de l'axe `i` sur `n` : -90° (en haut) pour le n° 0.
pub fn angle(i: usize, n: usize) -> f32 {
    -FRAC_PI_2 + TAU * i as f32 / n.max(1) as f32
}

/// Point situé à `fraction` (0 = centre, 1 = bout de l'axe) sur l'axe `i`.
pub fn point(centre: [f32; 2], rayon: f32, i: usize, n: usize, fraction: f32) -> [f32; 2] {
    let a = angle(i, n);
    let r = rayon * fraction.clamp(0.0, 1.0);
    [centre[0] + r * a.cos(), centre[1] + r * a.sin()]
}

/// Sommet de la toile (bout de l'axe `i`).
pub fn sommet(centre: [f32; 2], rayon: f32, i: usize, n: usize) -> [f32; 2] {
    point(centre, rayon, i, n, 1.0)
}

/// Les `n` sommets d'un polygone régulier de rayon `rayon` (une graduation de la toile).
pub fn toile(centre: [f32; 2], rayon: f32, n: usize) -> Vec<[f32; 2]> {
    (0..n).map(|i| sommet(centre, rayon, i, n)).collect()
}

/// Sommets du polygone des scores. Un axe sans donnée (`None`) est placé au
/// centre (le dessin l'indique par « ? » à côté de l'axe).
pub fn polygone(centre: [f32; 2], rayon: f32, scores: &[Option<u8>]) -> Vec<[f32; 2]> {
    let n = scores.len();
    scores
        .iter()
        .enumerate()
        .map(|(i, s)| point(centre, rayon, i, n, s.map_or(0.0, |v| v as f32 / 100.0)))
        .collect()
}

/// Comment accrocher le libellé d'un axe à son point (pour qu'il ne chevauche pas
/// la toile) : -1 = le texte part vers la gauche du point, 0 = centré, 1 = vers la
/// droite ; idem en vertical (-1 = au-dessus, 1 = en dessous).
pub fn ancrage(i: usize, n: usize) -> (i8, i8) {
    let a = angle(i, n);
    let signe = |v: f32| if v > 0.25 { 1 } else if v < -0.25 { -1 } else { 0 };
    (signe(a.cos()), signe(a.sin()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proche(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn sommets_de_l_heptagone() {
        let c = [100.0, 100.0];
        // Axe 0 tout en haut (y plus petit que le centre).
        assert!(proche(sommet(c, 50.0, 0, 7), [100.0, 50.0]));
        let t = toile(c, 50.0, 7);
        assert_eq!(t.len(), 7);
        // Tous à la même distance du centre.
        for p in &t {
            let d = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt();
            assert!((d - 50.0).abs() < 1e-3);
        }
        // Sens des aiguilles d'une montre : l'axe 1 est à droite, le dernier à gauche,
        // symétriques par rapport à la verticale.
        assert!(t[1][0] > c[0] && t[6][0] < c[0]);
        assert!((t[1][0] - c[0] + (t[6][0] - c[0])).abs() < 1e-3);
        assert!((t[1][1] - t[6][1]).abs() < 1e-3);
        // Axes 3 et 4 en bas.
        assert!(t[3][1] > c[1] && t[4][1] > c[1]);
    }

    #[test]
    fn polygone_des_scores() {
        let c = [0.0, 0.0];
        let p = polygone(c, 100.0, &[Some(100), Some(50), None, Some(0), Some(25), Some(75), Some(50)]);
        assert_eq!(p.len(), 7);
        assert!(proche(p[0], [0.0, -100.0]), "100 = bout de l'axe");
        assert!(proche(p[1], point(c, 100.0, 1, 7, 0.5)), "50 = mi-chemin");
        assert!(proche(p[2], c), "pas de donnée = centre");
        assert!(proche(p[3], c), "0 = centre");
        // Une valeur hors bornes reste sur la toile.
        assert!(proche(point(c, 10.0, 0, 7, 3.0), [0.0, -10.0]));
    }

    #[test]
    fn ancrage_des_libelles() {
        assert_eq!(ancrage(0, 7), (0, -1), "en haut : centré, au-dessus");
        assert_eq!(ancrage(1, 7).0, 1, "à droite : le texte part vers la droite");
        assert_eq!(ancrage(6, 7).0, -1);
        assert_eq!(ancrage(3, 7).1, 1, "en bas : en dessous");
    }
}
