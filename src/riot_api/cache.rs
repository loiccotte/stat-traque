//! Cache disque des réponses de l'API Riot.
//! Une partie terminée ne change JAMAIS : elle est gardée pour toujours
//! (0 requête la fois suivante). Les rangs, eux, expirent après quelques minutes.
//!
//! Un fichier par réponse dans `cache/riot/`, nommé par l'empreinte de l'URL.
//! 1re ligne : date d'expiration (secondes Unix, 0 = jamais). Reste : le JSON.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Cache {
    dossier: PathBuf,
}

/// Durée de vie d'une réponse.
#[derive(Debug, Clone, Copy)]
pub enum Duree {
    Toujours,
    Pendant(Duration),
}

fn maintenant() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Empreinte FNV-1a 64 bits : rapide, sans dépendance, suffisante pour nommer des fichiers.
fn empreinte(texte: &str) -> u64 {
    texte.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

impl Cache {
    pub fn nouveau(dossier: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dossier);
        Self { dossier }
    }

    fn chemin(&self, cle: &str) -> PathBuf {
        self.dossier.join(format!("{:016x}", empreinte(cle)))
    }

    /// Renvoie la réponse en cache si elle existe et n'a pas expiré.
    pub fn lire(&self, cle: &str) -> Option<String> {
        let contenu = std::fs::read_to_string(self.chemin(cle)).ok()?;
        let (expire, corps) = contenu.split_once('\n')?;
        let expire: u64 = expire.parse().ok()?;
        (expire == 0 || expire > maintenant()).then(|| corps.to_string())
    }

    pub fn ecrire(&self, cle: &str, corps: &str, duree: Duree) {
        let expire = match duree {
            Duree::Toujours => 0,
            Duree::Pendant(d) => maintenant() + d.as_secs(),
        };
        let _ = std::fs::write(self.chemin(cle), format!("{expire}\n{corps}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garde_et_expire() {
        let dossier = std::env::temp_dir().join(format!("stat-traque-test-{}", std::process::id()));
        let c = Cache::nouveau(dossier.clone());
        c.ecrire("/match/1", "{\"a\":1}", Duree::Toujours);
        c.ecrire("/rang/1", "[]", Duree::Pendant(Duration::ZERO));
        assert_eq!(c.lire("/match/1").as_deref(), Some("{\"a\":1}"));
        assert_eq!(c.lire("/rang/1"), None); // déjà expiré
        assert_eq!(c.lire("/inconnu"), None);
        let _ = std::fs::remove_dir_all(dossier);
    }
}
