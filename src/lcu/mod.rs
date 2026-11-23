//! Client LoL ("LCU" = League Client Update) : connexion locale.
//!
//! Principe :
//!  1. Le client LoL écrit un fichier `lockfile` dans son dossier d'installation.
//!     Une ligne : `nom:pid:PORT:MOT_DE_PASSE:protocole`.
//!  2. Avec ce port et ce mot de passe on lui parle en HTTPS (requêtes ponctuelles)
//!     et en WebSocket (il nous PRÉVIENT de chaque changement, voir `evenements.rs`).

pub mod actions;
pub mod evenements;
pub mod identite;

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

/// Où chercher le lockfile. Si le dossier du jeu est connu (mémorisé), on ne
/// regarde QUE là : une seule lecture de fichier, coût ~0.
/// Sinon : l'emplacement par défaut puis le processus lancé (voir plus bas).
/// On ne sonde pas D:, E:... : un lecteur réseau ou optique peut bloquer plusieurs secondes.
fn dossiers_candidats(dossier_connu: Option<&str>) -> Vec<PathBuf> {
    let mut d: Vec<PathBuf> = Vec::new();
    // Pour tester sans LoL : STAT_TRAQUE_LOCKFILE=chemin\du\dossier
    if let Some(test) = std::env::var_os("STAT_TRAQUE_LOCKFILE") {
        d.push(PathBuf::from(test));
    }
    match dossier_connu {
        Some(c) => d.push(PathBuf::from(c)),
        None => d.push(PathBuf::from("C:\\Riot Games\\League of Legends")),
    }
    d
}

/// Demande à Windows où tourne `LeagueClientUx.exe`, via la liste des processus
/// (CreateToolhelp32Snapshot) : environ 1 ms, aucun programme externe lancé.
#[cfg(windows)]
fn dossier_du_processus() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY : appels Windows documentés ; chaque poignée ouverte est refermée.
    unsafe {
        let liste = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if liste == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entree: PROCESSENTRY32W = std::mem::zeroed();
        entree.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut pid = None;
        let mut ok = Process32FirstW(liste, &mut entree) != 0;
        while ok {
            let fin = entree.szExeFile.iter().position(|&c| c == 0).unwrap_or(entree.szExeFile.len());
            let nom = String::from_utf16_lossy(&entree.szExeFile[..fin]);
            if nom.eq_ignore_ascii_case("LeagueClientUx.exe") {
                pid = Some(entree.th32ProcessID);
                break;
            }
            ok = Process32NextW(liste, &mut entree) != 0;
        }
        CloseHandle(liste);

        let processus = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid?);
        if processus.is_null() {
            return None;
        }
        let mut tampon = [0u16; 1024];
        let mut taille = tampon.len() as u32;
        let reussi = QueryFullProcessImageNameW(processus, 0, tampon.as_mut_ptr(), &mut taille) != 0;
        CloseHandle(processus);
        if !reussi {
            return None;
        }
        let chemin = PathBuf::from(std::ffi::OsString::from_wide(&tampon[..taille as usize]));
        chemin.parent().map(Path::to_path_buf)
    }
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
/// `scanner` : consulter aussi la liste des processus (utile si le dossier est
/// inconnu, ou si le jeu a été déplacé).
pub fn trouver_client(dossier_connu: Option<&str>, scanner: bool) -> Option<(Lcu, PathBuf)> {
    for dossier in dossiers_candidats(dossier_connu) {
        if let Some(lcu) = lire_lockfile(&dossier) {
            return Some((lcu, dossier));
        }
    }
    if scanner {
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
