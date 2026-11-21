//! Limiteur de débit : Riot coupe la clé si on dépasse ses limites.
//! Clé de développement : 20 requêtes par seconde ET 100 requêtes par 2 minutes.
//!
//! Principe des "fenêtres glissantes" : pour chaque limite, on garde l'heure des
//! dernières requêtes. Avant une requête, s'il y en a déjà trop dans la fenêtre,
//! on attend que la plus ancienne en sorte.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Fenetre {
    limite: usize,
    duree: Duration,
    appels: VecDeque<Instant>,
}

pub struct Limiteur {
    fenetres: Mutex<Vec<Fenetre>>,
    /// Si Riot répond 429 ("trop de requêtes"), plus rien avant cette heure-là.
    bloque_jusqua: Mutex<Option<Instant>>,
}

impl Limiteur {
    /// `limites` = liste de (nombre de requêtes, durée).
    pub fn nouveau(limites: &[(usize, Duration)]) -> Self {
        let fenetres = limites
            .iter()
            .map(|&(limite, duree)| Fenetre { limite, duree, appels: VecDeque::with_capacity(limite) })
            .collect();
        Self { fenetres: Mutex::new(fenetres), bloque_jusqua: Mutex::new(None) }
    }

    /// LE limiteur de l'app, partagé par tous : les limites Riot sont par clé,
    /// pas par objet. Deux limiteurs séparés laisseraient passer deux fois trop de requêtes.
    pub fn global() -> &'static Limiteur {
        static GLOBAL: std::sync::OnceLock<Limiteur> = std::sync::OnceLock::new();
        // Limites d'une clé de développement Riot.
        GLOBAL.get_or_init(|| Self::nouveau(&[(20, Duration::from_secs(1)), (100, Duration::from_secs(120))]))
    }

    /// Combien de temps attendre avant d'avoir le droit de faire une requête ?
    /// `Duration::ZERO` = tout de suite (et la requête est alors comptée).
    pub fn essayer(&self, maintenant: Instant) -> Duration {
        if let Some(fin) = *self.bloque_jusqua.lock().unwrap() {
            if fin > maintenant {
                return fin - maintenant;
            }
        }
        let mut fenetres = self.fenetres.lock().unwrap();
        let mut attente = Duration::ZERO;
        for f in fenetres.iter_mut() {
            // On oublie les requêtes sorties de la fenêtre.
            while f.appels.front().is_some_and(|&t| maintenant.duration_since(t) >= f.duree) {
                f.appels.pop_front();
            }
            if f.appels.len() >= f.limite {
                let plus_ancienne = f.appels[0];
                attente = attente.max(f.duree - maintenant.duration_since(plus_ancienne));
            }
        }
        if attente.is_zero() {
            for f in fenetres.iter_mut() {
                f.appels.push_back(maintenant);
            }
        }
        attente
    }

    /// Bloque le thread jusqu'à avoir le droit de faire une requête.
    pub fn attendre(&self) {
        loop {
            let attente = self.essayer(Instant::now());
            if attente.is_zero() {
                return;
            }
            std::thread::sleep(attente);
        }
    }

    /// Riot a répondu 429 : on se met en pause le temps demandé.
    pub fn pause(&self, duree: Duration) {
        *self.bloque_jusqua.lock().unwrap() = Some(Instant::now() + duree);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respecte_la_limite_courte_puis_la_longue() {
        let l = Limiteur::nouveau(&[(2, Duration::from_secs(1)), (3, Duration::from_secs(10))]);
        let t0 = Instant::now();
        assert!(l.essayer(t0).is_zero());
        assert!(l.essayer(t0).is_zero());
        // 3e requête dans la même seconde : il faut attendre 1 s.
        assert_eq!(l.essayer(t0), Duration::from_secs(1));
        // 1 s plus tard : OK (3e requête sur 10 s).
        assert!(l.essayer(t0 + Duration::from_secs(1)).is_zero());
        // 4e requête : la fenêtre de 10 s est pleine -> attendre jusqu'à t0 + 10 s.
        assert_eq!(l.essayer(t0 + Duration::from_secs(2)), Duration::from_secs(8));
        assert!(l.essayer(t0 + Duration::from_secs(10)).is_zero());
    }
}
