//! Mise à jour automatique depuis les « releases » GitHub du projet.
//!
//! Au démarrage, en arrière-plan, AU PLUS une fois par jour :
//!  1. on demande à GitHub la dernière version publiée (un petit JSON) ;
//!  2. si son numéro (`tag_name`, ex : "v0.4.0") est plus grand que le nôtre et
//!     qu'elle contient `stat-traque.exe`, on télécharge ce fichier à côté de l'exe
//!     sous le nom `stat-traque.exe.nouveau`, et on vérifie sa taille ;
//!  3. on renomme l'exe actuel en `stat-traque.exe.ancien` (Windows interdit de
//!     SUPPRIMER un exe en cours d'exécution, mais autorise de le RENOMMER),
//!     puis le nouveau en `stat-traque.exe` ;
//!  4. au lancement suivant, c'est la nouvelle version qui démarre, et elle
//!     supprime `stat-traque.exe.ancien`.
//!
//! Désactivable avec `maj=non` dans reglages.txt.
//! Coût : un thread qui vit quelques secondes puis disparaît ; le fichier est
//! écrit sur le disque au fur et à mesure (jamais entièrement en RAM).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::log;

/// Dernière version publiée (les releases « prerelease », comme `stats`, sont ignorées par GitHub).
const URL_DERNIERE: &str = "https://api.github.com/repos/loiccotte/stat-traque/releases/latest";
/// Nom du fichier attendu dans la release (voir .github/workflows/release.yml).
const NOM_ASSET: &str = "stat-traque.exe";
/// Garde-fous sur la taille : un exe tronqué ou une page d'erreur ne doivent pas passer.
const TAILLE_MIN: u64 = 100 * 1024;
const TAILLE_MAX: u64 = 32 * 1024 * 1024;
/// Pas plus d'une vérification par jour.
const INTERVALLE: Duration = Duration::from_secs(24 * 3600);

/// "v1.2.3" ou "1.2.3" -> (1, 2, 3). Tout autre format -> None. Fonction pure.
/// Les versions « 1.2.3-beta » sont refusées exprès : jamais de mise à jour vers une bêta.
pub fn version(texte: &str) -> Option<(u32, u32, u32)> {
    let texte = texte.trim();
    let texte = texte.strip_prefix('v').unwrap_or(texte);
    let mut morceaux = texte.split('.').map(|m| m.parse::<u32>().ok());
    let v = (morceaux.next()??, morceaux.next()??, morceaux.next()??);
    // Un 4e morceau ("1.2.3.4") : format inconnu.
    morceaux.next().is_none().then_some(v)
}

/// `tag` (ex : "v0.4.0") est-il STRICTEMENT plus récent que `actuelle` (ex : "0.3.0") ?
/// Les tuples se comparent dans l'ordre : majeur, puis mineur, puis correctif.
pub fn plus_recente(tag: &str, actuelle: &str) -> bool {
    match (version(tag), version(actuelle)) {
        (Some(t), Some(a)) => t > a,
        _ => false,
    }
}

/// Ce qu'il faut télécharger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseAJour {
    pub version: String,
    pub url: String,
    pub taille: u64,
}

/// Lit la réponse de GitHub : une mise à jour à installer, ou rien. Fonction pure.
pub fn choisir(release: &Value, actuelle: &str) -> Option<MiseAJour> {
    let tag = release["tag_name"].as_str()?;
    if !plus_recente(tag, actuelle) || release["draft"] == true || release["prerelease"] == true {
        return None;
    }
    let asset = release["assets"].as_array()?.iter().find(|a| a["name"] == NOM_ASSET)?;
    let taille = asset["size"].as_u64().filter(|t| (TAILLE_MIN..=TAILLE_MAX).contains(t))?;
    Some(MiseAJour {
        version: tag.to_string(),
        url: asset["browser_download_url"].as_str()?.to_string(),
        taille,
    })
}

/// `stat-traque.exe` -> (`stat-traque.exe.nouveau`, `stat-traque.exe.ancien`).
fn chemins(exe: &Path) -> (PathBuf, PathBuf) {
    let nom = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| NOM_ASSET.into());
    (exe.with_file_name(format!("{nom}.nouveau")), exe.with_file_name(format!("{nom}.ancien")))
}

/// Faut-il demander à GitHub ? Oui si jamais fait ou si la dernière fois date d'au moins un jour.
fn doit_verifier(age_derniere_verification: Option<Duration>) -> bool {
    age_derniere_verification.is_none_or(|age| age >= INTERVALLE)
}

/// Appelée une fois au démarrage (après le contrôle « une seule instance »).
pub fn lancer_en_fond(reglages: &crate::stockage::Reglages) {
    let actif = reglages.get("maj") != Some("non");
    let _ = std::thread::Builder::new()
        .name("mise-a-jour".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let Ok(exe) = std::env::current_exe() else { return };
            // Restes d'une mise à jour précédente : l'ancien exe ne tourne plus, on le supprime.
            let (nouveau, ancien) = chemins(&exe);
            if std::fs::remove_file(&ancien).is_ok() {
                log!("Mise à jour : ancienne version supprimée, stat-traque {} actif.", env!("CARGO_PKG_VERSION"));
            }
            let _ = std::fs::remove_file(&nouveau);
            // Pas de mise à jour : si désactivée, hors Windows (l'exe publié est pour Windows),
            // ou en compilation de développement (on ne remplace pas target/debug).
            if !actif || !cfg!(windows) || cfg!(debug_assertions) {
                return;
            }
            if let Err(e) = verifier(&exe) {
                log!("Mise à jour : {e}");
            }
        });
}

fn verifier(exe: &Path) -> Result<(), String> {
    // La date de la dernière vérification = date de modification d'un petit fichier.
    let repere = crate::stockage::dossier_app().join("maj.txt");
    let age = std::fs::metadata(&repere).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if !doit_verifier(age) {
        return Ok(());
    }
    let release: Value = crate::http::internet()
        .get(URL_DERNIERE)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("GitHub injoignable ({e})"))?
        .into_json()
        .map_err(|e| format!("réponse GitHub illisible ({e})"))?;
    // Vérification faite (même s'il n'y a rien de neuf) : prochaine dans 24 h.
    let _ = std::fs::write(&repere, env!("CARGO_PKG_VERSION"));
    let Some(maj) = choisir(&release, env!("CARGO_PKG_VERSION")) else { return Ok(()) };
    log!("Mise à jour : version {} disponible, téléchargement...", maj.version);
    installer(&maj, exe)?;
    log!("Mise à jour {} installée, active au prochain lancement.", maj.version);
    Ok(())
}

/// Télécharge à côté de l'exe, vérifie, puis échange les fichiers.
fn installer(maj: &MiseAJour, exe: &Path) -> Result<(), String> {
    let (nouveau, ancien) = chemins(exe);
    if let Err(e) = telecharger(maj, &nouveau) {
        let _ = std::fs::remove_file(&nouveau);
        return Err(e);
    }
    // Échange : exe -> .ancien, puis .nouveau -> exe. Si la 2e étape rate, on remet tout.
    let _ = std::fs::remove_file(&ancien);
    std::fs::rename(exe, &ancien).map_err(|e| format!("impossible de renommer l'exe ({e})"))?;
    if let Err(e) = std::fs::rename(&nouveau, exe) {
        let _ = std::fs::rename(&ancien, exe);
        let _ = std::fs::remove_file(&nouveau);
        return Err(format!("impossible d'installer la nouvelle version ({e})"));
    }
    Ok(())
}

fn telecharger(maj: &MiseAJour, destination: &Path) -> Result<(), String> {
    // Délai plus long que d'habitude : un fichier de ~1 Mo sur une connexion lente.
    let reponse = crate::http::internet()
        .get(&maj.url)
        .timeout(Duration::from_secs(120))
        .call()
        .map_err(|e| format!("téléchargement impossible ({e})"))?;
    let mut fichier = std::fs::File::create(destination).map_err(|e| format!("écriture impossible ({e})"))?;
    // `take` : on n'écrit jamais plus que TAILLE_MAX (+1 pour détecter le dépassement).
    let mut lecteur = reponse.into_reader().take(TAILLE_MAX + 1);
    // On lit les 2 premiers octets à part pour vérifier la signature d'un exe Windows.
    let mut debut = [0u8; 2];
    lecteur.read_exact(&mut debut).map_err(|e| format!("téléchargement interrompu ({e})"))?;
    fichier.write_all(&debut).map_err(|e| e.to_string())?;
    let recu = 2 + std::io::copy(&mut lecteur, &mut fichier).map_err(|e| format!("téléchargement interrompu ({e})"))?;
    fichier.sync_all().map_err(|e| e.to_string())?;
    verifier_fichier(&debut, recu, maj.taille)
}

/// Le fichier reçu ressemble-t-il à l'exe annoncé ? Fonction pure.
/// Tout exe Windows commence par les lettres "MZ".
fn verifier_fichier(debut: &[u8], recu: u64, attendu: u64) -> Result<(), String> {
    if recu != attendu {
        return Err(format!("taille reçue {recu} octets, {attendu} attendus : fichier ignoré"));
    }
    if debut != b"MZ" {
        return Err("le fichier reçu n'est pas un exe Windows : ignoré".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lit_les_numeros_de_version() {
        assert_eq!(version("v0.3.0"), Some((0, 3, 0)));
        assert_eq!(version("1.12.7"), Some((1, 12, 7)));
        assert_eq!(version("stats"), None);
        assert_eq!(version("v1.2"), None);
        assert_eq!(version("v1.2.3.4"), None);
        assert_eq!(version("v1.2.3-beta"), None);
    }

    #[test]
    fn compare_les_versions() {
        assert!(plus_recente("v0.3.1", "0.3.0"));
        assert!(plus_recente("v0.10.0", "0.9.9")); // 10 > 9 : comparaison de nombres, pas de texte
        assert!(plus_recente("v1.0.0", "0.99.99"));
        assert!(!plus_recente("v0.3.0", "0.3.0")); // même version : rien à faire
        assert!(!plus_recente("v0.2.9", "0.3.0")); // jamais de retour en arrière
        assert!(!plus_recente("stats", "0.3.0"));
    }

    fn release(tag: &str, nom: &str, taille: u64) -> Value {
        json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": [
            {"name": "stat-traque.zip", "size": 500_000, "browser_download_url": "https://x/zip"},
            {"name": nom, "size": taille, "browser_download_url": "https://x/exe"}]})
    }

    #[test]
    fn choisit_l_exe_de_la_nouvelle_version() {
        let maj = choisir(&release("v0.4.0", "stat-traque.exe", 900_000), "0.3.0").unwrap();
        assert_eq!(maj, MiseAJour { version: "v0.4.0".into(), url: "https://x/exe".into(), taille: 900_000 });
        // Pas plus récente, pas d'exe, taille absurde : rien.
        assert!(choisir(&release("v0.3.0", "stat-traque.exe", 900_000), "0.3.0").is_none());
        assert!(choisir(&release("v0.4.0", "autre.exe", 900_000), "0.3.0").is_none());
        assert!(choisir(&release("v0.4.0", "stat-traque.exe", 10), "0.3.0").is_none());
        assert!(choisir(&json!({"message": "Not Found"}), "0.3.0").is_none());
    }

    #[test]
    fn noms_des_fichiers_a_cote_de_l_exe() {
        let (nouveau, ancien) = chemins(Path::new("/jeux/stat-traque.exe"));
        assert_eq!(nouveau, Path::new("/jeux/stat-traque.exe.nouveau"));
        assert_eq!(ancien, Path::new("/jeux/stat-traque.exe.ancien"));
    }

    #[test]
    fn une_verification_par_jour() {
        assert!(doit_verifier(None));
        assert!(!doit_verifier(Some(Duration::from_secs(3600))));
        assert!(doit_verifier(Some(Duration::from_secs(25 * 3600))));
    }

    #[test]
    fn refuse_un_fichier_douteux() {
        assert!(verifier_fichier(b"MZ", 900_000, 900_000).is_ok());
        assert!(verifier_fichier(b"MZ", 899_999, 900_000).is_err());
        assert!(verifier_fichier(b"<h", 900_000, 900_000).is_err());
    }
}
