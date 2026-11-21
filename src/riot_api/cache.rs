//! Cache disque des réponses de l'API Riot.
//! Une partie terminée ne change JAMAIS : elle est gardée pour toujours
//! (0 requête la fois suivante). Les rangs, eux, expirent après quelques minutes.
//!
//! Un fichier par réponse dans `cache/riot/`, nommé par l'empreinte de l'URL.
//! 1re ligne : date d'expiration (secondes Unix, 0 = jamais). Reste : le JSON.
//!
//! Pour que le dossier ne grossisse pas sans fin, `nettoyer()` (au démarrage) supprime
//! les réponses expirées et les parties non relues depuis `GARDER_PARTIES_JOURS` jours.
//! (La base locale des parties, T19, gardera ensuite un résumé compact de CHAQUE partie.)

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Les parties en cache « pour toujours » non utilisées depuis ce délai sont supprimées.
const GARDER_PARTIES_JOURS: u64 = 30;

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

    /// Écrit une réponse. Durée nulle = rien n'est écrit (ex : vérification de la clé).
    /// Écriture « atomique » : fichier temporaire puis renommage, donc jamais de
    /// fichier à moitié écrit si l'app est coupée au mauvais moment.
    pub fn ecrire(&self, cle: &str, corps: &str, duree: Duree) {
        let expire = match duree {
            Duree::Toujours => 0,
            Duree::Pendant(d) if d.is_zero() => return,
            Duree::Pendant(d) => maintenant() + d.as_secs(),
        };
        let chemin = self.chemin(cle);
        let temporaire = chemin.with_extension("tmp");
        if std::fs::write(&temporaire, format!("{expire}\n{corps}")).is_ok() {
            let _ = std::fs::rename(&temporaire, &chemin);
        }
    }

    /// Supprime une réponse (ex : fichier abîmé).
    pub fn oublier(&self, cle: &str) {
        let _ = std::fs::remove_file(self.chemin(cle));
    }

    /// Ménage : réponses expirées, fichiers temporaires, vieilles parties.
    /// Ne lit que la 1re ligne de chaque fichier. Renvoie le nombre de fichiers supprimés.
    pub fn nettoyer(&self) -> usize {
        use std::io::BufRead;
        let limite_parties = Duration::from_secs(GARDER_PARTIES_JOURS * 24 * 3600);
        let mut supprimes = 0;
        for entree in std::fs::read_dir(&self.dossier).into_iter().flatten().flatten() {
            let chemin = entree.path();
            let a_supprimer = if chemin.extension().is_some_and(|e| e == "tmp") {
                true
            } else {
                let mut premiere = String::new();
                let lu = std::fs::File::open(&chemin)
                    .map(|f| std::io::BufReader::new(f).read_line(&mut premiere))
                    .is_ok();
                match premiere.trim().parse::<u64>() {
                    Ok(0) => entree
                        .metadata()
                        .and_then(|m| m.accessed().or_else(|_| m.modified()))
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > limite_parties),
                    Ok(expire) => expire <= maintenant(),
                    Err(_) => lu, // fichier illisible ou abîmé
                }
            };
            if a_supprimer && std::fs::remove_file(&chemin).is_ok() {
                supprimes += 1;
            }
        }
        supprimes
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
        c.ecrire("/statut", "{}", Duree::Pendant(Duration::ZERO));
        assert_eq!(c.lire("/match/1").as_deref(), Some("{\"a\":1}"));
        assert_eq!(c.lire("/statut"), None); // durée nulle : jamais écrit
        assert_eq!(c.lire("/inconnu"), None);
        // Une réponse expirée est supprimée par le ménage, la partie reste.
        std::fs::write(c.chemin("/rang/1"), "1\n[]").unwrap();
        assert_eq!(c.nettoyer(), 1);
        assert!(c.lire("/match/1").is_some());
        let _ = std::fs::remove_dir_all(dossier);
    }
}
