//! Stockage local : tout ce que l'app garde sur le disque est dans UN dossier,
//! `%APPDATA%\stat-traque\` sous Windows (`~/.config/stat-traque/` ailleurs).
//!
//!   cle.txt        la clé API Riot (jamais dans le dépôt)
//!   reglages.txt   les réglages, une ligne `nom=valeur` par réglage
//!   journal.txt    le journal de la dernière session
//!   cache/         Data Dragon et réponses de l'API Riot

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Dossier racine de l'app (créé s'il n'existe pas).
pub fn dossier_app() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    let dossier = base.join("stat-traque");
    let _ = std::fs::create_dir_all(&dossier);
    dossier
}

/// Sous-dossier du cache (ex : `dossier_cache("ddragon")`).
pub fn dossier_cache(nom: &str) -> PathBuf {
    let d = dossier_app().join("cache").join(nom);
    let _ = std::fs::create_dir_all(&d);
    d
}

// ---------- Clé API ----------

pub fn chemin_cle() -> PathBuf {
    dossier_app().join("cle.txt")
}

/// La clé : variable d'environnement `STAT_TRAQUE_CLE` (collecte automatique sur
/// GitHub, où la clé est un secret), sinon le fichier cle.txt.
pub fn lire_cle() -> Option<String> {
    if let Ok(cle) = std::env::var("STAT_TRAQUE_CLE") {
        if !cle.trim().is_empty() {
            return Some(cle.trim().to_string());
        }
    }
    let cle = std::fs::read_to_string(chemin_cle()).ok()?;
    let cle = cle.trim();
    (!cle.is_empty()).then(|| cle.to_string())
}

pub fn sauver_cle(cle: &str) -> std::io::Result<()> {
    std::fs::write(chemin_cle(), cle.trim())
}

// ---------- Réglages ----------
// Format volontairement simple (pas de dépendance TOML/serde) :
//   region=euw1
//   dossier_lol=C:\Riot Games\League of Legends

/// Les réglages en mémoire. `BTreeMap` = dictionnaire trié (fichier stable).
#[derive(Debug, Default, Clone)]
pub struct Reglages {
    valeurs: BTreeMap<String, String>,
}

impl Reglages {
    fn chemin() -> PathBuf {
        dossier_app().join("reglages.txt")
    }

    /// Lit le fichier ; s'il n'existe pas, réglages vides (valeurs par défaut).
    pub fn charger() -> Self {
        let texte = std::fs::read_to_string(Self::chemin()).unwrap_or_default();
        Self::depuis_texte(&texte)
    }

    pub fn depuis_texte(texte: &str) -> Self {
        let valeurs = texte
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        Self { valeurs }
    }

    pub fn sauver(&self) -> std::io::Result<()> {
        let texte: String = self.valeurs.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        std::fs::write(Self::chemin(), texte)
    }

    pub fn get(&self, nom: &str) -> Option<&str> {
        self.valeurs.get(nom).map(String::as_str)
    }

    pub fn set(&mut self, nom: &str, valeur: &str) {
        self.valeurs.insert(nom.to_string(), valeur.to_string());
    }

    /// Serveur Riot (plateforme). EUW par défaut.
    pub fn region(&self) -> &str {
        self.get("region").unwrap_or("euw1")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_les_reglages_et_ignore_les_commentaires() {
        let r = Reglages::depuis_texte("# commentaire\nregion = kr\ndossier_lol=C:\\LoL\nligne sans egal\n");
        assert_eq!(r.region(), "kr");
        assert_eq!(r.get("dossier_lol"), Some("C:\\LoL"));
        assert_eq!(r.get("ligne sans egal"), None);
    }

    #[test]
    fn region_par_defaut() {
        assert_eq!(Reglages::default().region(), "euw1");
    }
}
