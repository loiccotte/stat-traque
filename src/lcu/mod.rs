//! Client LoL ("LCU" = League Client Update) : connexion locale.
//!
//! Principe :
//!  1. Le client LoL écrit un fichier `lockfile` dans son dossier d'installation.
//!     Une ligne : `nom:pid:PORT:MOT_DE_PASSE:protocole`.
//!  2. Avec ce port et ce mot de passe on lui parle en HTTPS (requêtes ponctuelles)
//!     et en WebSocket (il nous PRÉVIENT de chaque changement, voir `evenements.rs`).

pub mod evenements;

use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;

/// Les infos pour se connecter au client LoL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lcu {
    pub port: u16,
    /// En-tête `Authorization` déjà prêt (`Basic base64(riot:MOT_DE_PASSE)`).
    pub auth: String,
}

impl Lcu {
    /// Découpe le contenu du lockfile. `None` si le format est inattendu.
    pub fn depuis_lockfile(contenu: &str) -> Option<Self> {
        let parties: Vec<&str> = contenu.trim().split(':').collect();
        if parties.len() < 5 {
            return None;
        }
        let port = parties[2].parse().ok()?;
        let auth = format!("Basic {}", STANDARD.encode(format!("riot:{}", parties[3])));
        Some(Lcu { port, auth })
    }

    /// GET sur le client, réponse JSON. `None` si erreur (ex : pas en champ select).
    pub fn get(&self, chemin: &str) -> Option<Value> {
        crate::http::local()
            .get(&format!("https://127.0.0.1:{}{chemin}", self.port))
            .set("Authorization", &self.auth)
            .call()
            .ok()?
            .into_json()
            .ok()
    }
}

/// Dossiers où chercher le lockfile, du plus probable au moins probable.
/// `dossier_connu` = dossier mémorisé dans les réglages lors d'une détection précédente.
fn dossiers_candidats(dossier_connu: Option<&str>) -> Vec<PathBuf> {
    let mut d: Vec<PathBuf> = Vec::new();
    // Pour tester sans LoL (ou sous Linux) : STAT_TRAQUE_LOCKFILE=chemin/du/dossier
    if let Some(test) = std::env::var_os("STAT_TRAQUE_LOCKFILE") {
        d.push(PathBuf::from(test));
    }
    if let Some(c) = dossier_connu {
        d.push(PathBuf::from(c));
    }
    for lettre in ["C", "D", "E", "F"] {
        d.push(PathBuf::from(format!("{lettre}:\\Riot Games\\League of Legends")));
    }
    d
}

/// Demande à Windows où tourne `LeagueClientUx.exe`.
/// COÛTEUX (lance PowerShell, ~200 ms et ~40 Mo le temps de l'appel) :
/// on ne l'appelle qu'en dernier recours, puis on mémorise le dossier trouvé.
#[cfg(windows)]
fn dossier_du_processus() -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000; // pas de fenêtre noire qui clignote
    let sortie = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", "(Get-Process LeagueClientUx -ErrorAction SilentlyContinue).Path"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let chemin = String::from_utf8_lossy(&sortie.stdout).trim().to_string();
    Path::new(&chemin).parent().map(Path::to_path_buf).filter(|p| !p.as_os_str().is_empty())
}

#[cfg(not(windows))]
fn dossier_du_processus() -> Option<PathBuf> {
    None
}

fn lire_lockfile(dossier: &Path) -> Option<Lcu> {
    let contenu = std::fs::read_to_string(dossier.join("lockfile")).ok()?;
    Lcu::depuis_lockfile(&contenu)
}

/// Cherche le client. Renvoie la connexion et le dossier où il a été trouvé.
/// `avec_processus` autorise l'appel PowerShell (à faire rarement).
pub fn trouver_client(dossier_connu: Option<&str>, avec_processus: bool) -> Option<(Lcu, PathBuf)> {
    for dossier in dossiers_candidats(dossier_connu) {
        if let Some(lcu) = lire_lockfile(&dossier) {
            return Some((lcu, dossier));
        }
    }
    if avec_processus {
        let dossier = dossier_du_processus()?;
        return lire_lockfile(&dossier).map(|l| (l, dossier));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_un_lockfile() {
        let lcu = Lcu::depuis_lockfile("LeagueClient:1234:54321:secret:https\n").unwrap();
        assert_eq!(lcu.port, 54321);
        // base64("riot:secret") = cmlvdDpzZWNyZXQ=
        assert_eq!(lcu.auth, "Basic cmlvdDpzZWNyZXQ=");
    }

    #[test]
    fn refuse_un_lockfile_abime() {
        assert!(Lcu::depuis_lockfile("").is_none());
        assert!(Lcu::depuis_lockfile("a:b:pasunport:d:e").is_none());
    }
}
