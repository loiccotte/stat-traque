//! Mise à jour automatique depuis les « releases » GitHub du projet.
//!
//! Au démarrage, en arrière-plan, AU PLUS une fois par jour (même en cas d'échec) :
//!  1. on demande à GitHub la dernière version publiée (un petit JSON) ;
//!  2. si son numéro (`tag_name`, ex : "v0.4.0") est plus grand que le nôtre et
//!     qu'elle contient `stat-traque.exe`, on télécharge ce fichier à côté de l'exe
//!     sous le nom `stat-traque.exe.nouveau`, en vérifiant l'adresse, la taille,
//!     la signature "MZ" et l'empreinte SHA-256 annoncée par GitHub (`digest`) ;
//!  3. on renomme l'exe actuel en `stat-traque.exe.ancien` (Windows interdit de
//!     SUPPRIMER un exe en cours d'exécution, mais autorise de le RENOMMER),
//!     puis le nouveau en `stat-traque.exe` ;
//!  4. au lancement suivant, c'est la nouvelle version qui démarre. On GARDE
//!     `stat-traque.exe.ancien` pour pouvoir revenir en arrière : il n'est supprimé
//!     qu'au lancement d'APRÈS (la nouvelle version a donc déjà démarré une fois).
//!
//! Retour arrière à la main : supprimer stat-traque.exe et renommer
//! stat-traque.exe.ancien en stat-traque.exe (puis `maj=non` dans reglages.txt).
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
/// Seules adresses de téléchargement acceptées : les fichiers des releases DE CE DÉPÔT.
const PREFIXE_TELECHARGEMENT: &str = "https://github.com/loiccotte/stat-traque/releases/download/";
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

/// L'adresse pointe-t-elle bien vers un fichier de release de CE dépôt ? Fonction pure.
/// (Une réponse falsifiée ne doit pas pouvoir nous faire télécharger n'importe quoi.)
pub fn url_autorisee(url: &str) -> bool {
    url.strip_prefix(PREFIXE_TELECHARGEMENT)
        .is_some_and(|reste| !reste.is_empty() && !reste.contains("..") && !reste.contains(['?', '#', '@', '\\']))
}

/// Ce qu'il faut télécharger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseAJour {
    pub version: String,
    pub url: String,
    pub taille: u64,
    /// Empreinte annoncée par GitHub, ex : "sha256:9f86d0..." (absente sur les vieilles releases).
    pub digest: Option<String>,
}

/// Lit la réponse de GitHub : une mise à jour à installer, ou rien. Fonction pure.
pub fn choisir(release: &Value, actuelle: &str) -> Option<MiseAJour> {
    let tag = release["tag_name"].as_str()?;
    if !plus_recente(tag, actuelle) || release["draft"] == true || release["prerelease"] == true {
        return None;
    }
    let asset = release["assets"].as_array()?.iter().find(|a| a["name"] == NOM_ASSET)?;
    let taille = asset["size"].as_u64().filter(|t| (TAILLE_MIN..=TAILLE_MAX).contains(t))?;
    let url = asset["browser_download_url"].as_str().filter(|u| url_autorisee(u))?;
    Some(MiseAJour {
        version: tag.to_string(),
        url: url.to_string(),
        taille,
        digest: asset["digest"].as_str().map(String::from),
    })
}

/// Octets -> texte hexadécimal en minuscules ("9f86d0..."). Fonction pure.
fn hexa(octets: &[u8]) -> String {
    octets.iter().map(|o| format!("{o:02x}")).collect()
}

/// Compare l'empreinte annoncée (`digest`) à celle calculée. Fonction pure.
/// Pas de digest annoncé : on se contente des autres vérifications.
/// Digest dans un format inconnu : on refuse (impossible de vérifier).
fn verifier_digest(annonce: Option<&str>, calcule: &[u8; 32]) -> Result<(), String> {
    let Some(annonce) = annonce else { return Ok(()) };
    let Some(attendu) = annonce.strip_prefix("sha256:") else {
        return Err(format!("empreinte dans un format inconnu ({annonce}) : fichier ignoré"));
    };
    if attendu.eq_ignore_ascii_case(&hexa(calcule)) {
        Ok(())
    } else {
        Err("empreinte SHA-256 différente de celle annoncée par GitHub : fichier ignoré".into())
    }
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

/// Que faire de `stat-traque.exe.ancien` à ce lancement ?
#[derive(Debug, PartialEq, Eq)]
enum Ancien {
    /// Pas d'ancien exe : rien à faire.
    Rien,
    /// 1er lancement de la nouvelle version : on le garde (retour arrière possible)
    /// et on note que cette version a démarré.
    GarderEtNoter,
    /// La version actuelle a déjà démarré au moins une fois : on peut le supprimer.
    Supprimer,
}

/// Fonction pure. `version_notee` : contenu du fichier `maj-demarree.txt`.
fn que_faire_de_l_ancien(ancien_existe: bool, version_notee: Option<&str>, actuelle: &str) -> Ancien {
    match (ancien_existe, version_notee.map(str::trim)) {
        (false, _) => Ancien::Rien,
        (true, Some(v)) if v == actuelle => Ancien::Supprimer,
        (true, _) => Ancien::GarderEtNoter,
    }
}

/// Appelée une fois au démarrage, APRÈS « Prêt » (l'app a démarré correctement).
pub fn lancer_en_fond(reglages: &crate::stockage::Reglages) {
    let actif = reglages.get("maj") != Some("non");
    let _ = std::thread::Builder::new()
        .name("mise-a-jour".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let Ok(exe) = std::env::current_exe() else { return };
            let (nouveau, ancien) = chemins(&exe);
            let _ = std::fs::remove_file(&nouveau); // téléchargement interrompu la dernière fois
            ranger_l_ancien(&ancien);
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

fn ranger_l_ancien(ancien: &Path) {
    let note = crate::stockage::dossier_app().join("maj-demarree.txt");
    let version_notee = std::fs::read_to_string(&note).ok();
    let actuelle = env!("CARGO_PKG_VERSION");
    match que_faire_de_l_ancien(ancien.exists(), version_notee.as_deref(), actuelle) {
        Ancien::Rien => {}
        Ancien::GarderEtNoter => {
            let _ = std::fs::write(&note, actuelle);
            log!("Mise à jour : stat-traque {actuelle} actif (ancienne version gardée jusqu'au prochain lancement).");
        }
        Ancien::Supprimer => {
            if std::fs::remove_file(ancien).is_ok() {
                let _ = std::fs::remove_file(&note);
            }
        }
    }
}

fn verifier(exe: &Path) -> Result<(), String> {
    // La date de la dernière vérification = date de modification d'un petit fichier.
    let repere = crate::stockage::dossier_app().join("maj.txt");
    let age = std::fs::metadata(&repere).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if !doit_verifier(age) {
        return Ok(());
    }
    // Noté AVANT la requête : réussite, 404 ou panne réseau, une seule tentative par jour.
    let _ = std::fs::write(&repere, env!("CARGO_PKG_VERSION"));
    let reponse = crate::http::internet()
        .get(URL_DERNIERE)
        .set("Accept", "application/vnd.github+json")
        .call();
    let release: Value = match reponse {
        Ok(r) => r.into_json().map_err(|e| format!("réponse GitHub illisible ({e})"))?,
        // 404 = le dépôt n'a encore aucune version publiée : rien d'anormal.
        Err(ureq::Error::Status(404, _)) => {
            log!("Mise à jour : aucune version publiée pour l'instant.");
            return Ok(());
        }
        Err(e) => return Err(format!("GitHub injoignable ({e}), nouvel essai demain")),
    };
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
    renommer(exe, &ancien).map_err(|e| format!("impossible de renommer l'exe ({e})"))?;
    if let Err(e) = renommer(&nouveau, exe) {
        let _ = renommer(&ancien, exe);
        let _ = std::fs::remove_file(&nouveau);
        return Err(format!("impossible d'installer la nouvelle version ({e})"));
    }
    Ok(())
}

/// Renommage DURABLE : sous Windows, `MoveFileExW` avec WRITE_THROUGH ne rend la main
/// qu'une fois le changement écrit sur le disque (une coupure de courant juste après
/// ne laisse pas un dossier sans exe). REPLACE_EXISTING écrase un vieux `.ancien`.
#[cfg(windows)]
fn renommer(de: &Path, vers: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH};
    let utf16 = |p: &Path| p.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (de, vers) = (utf16(de), utf16(vers));
    // SAFETY : deux chaînes terminées par un zéro, valides pendant l'appel.
    let ok = unsafe { MoveFileExW(de.as_ptr(), vers.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
    if ok != 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

#[cfg(not(windows))]
fn renommer(de: &Path, vers: &Path) -> std::io::Result<()> {
    std::fs::rename(de, vers)
}

fn telecharger(maj: &MiseAJour, destination: &Path) -> Result<(), String> {
    // Délai plus long que d'habitude : un fichier de ~1 Mo sur une connexion lente.
    let reponse = crate::http::internet()
        .get(&maj.url)
        .timeout(Duration::from_secs(120))
        .call()
        .map_err(|e| format!("téléchargement impossible ({e})"))?;
    let mut fichier = std::fs::File::create(destination).map_err(|e| format!("écriture impossible ({e})"))?;
    // `take` : on ne lit jamais plus que TAILLE_MAX (+1 pour détecter le dépassement).
    let mut lecteur = reponse.into_reader().take(TAILLE_MAX + 1);
    let mut empreinte = sha256::Sha256::nouveau()?;
    // On copie par morceaux de 64 Ko : écriture sur le disque + calcul de l'empreinte.
    let mut tampon = vec![0u8; 64 * 1024];
    let mut debut = Vec::with_capacity(2);
    let mut recu: u64 = 0;
    loop {
        let n = lecteur.read(&mut tampon).map_err(|e| format!("téléchargement interrompu ({e})"))?;
        if n == 0 {
            break;
        }
        let morceau = &tampon[..n];
        if debut.len() < 2 {
            debut.extend(morceau.iter().take(2 - debut.len()));
        }
        fichier.write_all(morceau).map_err(|e| format!("écriture impossible ({e})"))?;
        empreinte.ajouter(morceau)?;
        recu += n as u64;
    }
    fichier.sync_all().map_err(|e| e.to_string())?;
    verifier_fichier(&debut, recu, maj.taille)?;
    verifier_digest(maj.digest.as_deref(), &empreinte.finir()?)
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

/// SHA-256 calculé par Windows (BCrypt, dans bcrypt.dll) : aucune bibliothèque à embarquer.
#[cfg(windows)]
mod sha256 {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData, BCRYPT_HASH_HANDLE,
        BCRYPT_SHA256_ALG_HANDLE,
    };

    /// Calcul « au fil de l'eau » : on donne les morceaux un par un, puis on demande le résultat.
    pub struct Sha256(BCRYPT_HASH_HANDLE);

    impl Sha256 {
        pub fn nouveau() -> Result<Self, String> {
            let mut poignee: BCRYPT_HASH_HANDLE = std::ptr::null_mut();
            // SAFETY : BCRYPT_SHA256_ALG_HANDLE est une poignée prête à l'emploi (Windows 10+) ;
            // objet de calcul alloué par Windows (pointeur nul + taille 0), pas de clé.
            let code = unsafe {
                BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &mut poignee, std::ptr::null_mut(), 0, std::ptr::null(), 0, 0)
            };
            if code == 0 { Ok(Self(poignee)) } else { Err(format!("SHA-256 indisponible ({code:#x})")) }
        }

        pub fn ajouter(&mut self, octets: &[u8]) -> Result<(), String> {
            // SAFETY : la poignée est valide (créée par `nouveau`) et `octets` aussi.
            let code = unsafe { BCryptHashData(self.0, octets.as_ptr(), octets.len() as u32, 0) };
            if code == 0 { Ok(()) } else { Err(format!("calcul SHA-256 impossible ({code:#x})")) }
        }

        pub fn finir(self) -> Result<[u8; 32], String> {
            let mut resultat = [0u8; 32];
            // SAFETY : tampon de 32 octets = taille d'un SHA-256.
            let code = unsafe { BCryptFinishHash(self.0, resultat.as_mut_ptr(), 32, 0) };
            if code == 0 { Ok(resultat) } else { Err(format!("calcul SHA-256 impossible ({code:#x})")) }
        }
    }

    impl Drop for Sha256 {
        fn drop(&mut self) {
            // SAFETY : poignée créée par BCryptCreateHash, détruite une seule fois.
            unsafe { BCryptDestroyHash(self.0) };
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn empreinte_connue() {
            let mut h = super::Sha256::nouveau().unwrap();
            h.ajouter(b"ab").unwrap();
            h.ajouter(b"c").unwrap();
            assert_eq!(
                super::super::hexa(&h.finir().unwrap()),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            );
        }
    }
}

/// Hors Windows, la mise à jour est désactivée : ce calcul n'est jamais appelé.
#[cfg(not(windows))]
mod sha256 {
    pub struct Sha256;
    impl Sha256 {
        pub fn nouveau() -> Result<Self, String> {
            Err("SHA-256 : seulement sous Windows".into())
        }
        pub fn ajouter(&mut self, _octets: &[u8]) -> Result<(), String> {
            Ok(())
        }
        pub fn finir(self) -> Result<[u8; 32], String> {
            Err("SHA-256 : seulement sous Windows".into())
        }
    }
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

    #[test]
    fn n_accepte_que_les_fichiers_de_ce_depot() {
        let bon = "https://github.com/loiccotte/stat-traque/releases/download/v0.4.0/stat-traque.exe";
        assert!(url_autorisee(bon));
        assert!(!url_autorisee("https://github.com/pirate/stat-traque/releases/download/v0.4.0/stat-traque.exe"));
        assert!(!url_autorisee("http://github.com/loiccotte/stat-traque/releases/download/v0.4.0/stat-traque.exe"));
        assert!(!url_autorisee("https://github.com.pirate.net/loiccotte/stat-traque/releases/download/x"));
        assert!(!url_autorisee("https://github.com/loiccotte/stat-traque/releases/download/../../pirate/x.exe"));
        assert!(!url_autorisee("https://github.com/loiccotte/stat-traque/releases/download/"));
    }

    const URL: &str = "https://github.com/loiccotte/stat-traque/releases/download/v0.4.0/stat-traque.exe";

    fn release(tag: &str, nom: &str, taille: u64) -> Value {
        json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": [
            {"name": "stat-traque.zip", "size": 500_000, "browser_download_url": "https://x/zip"},
            {"name": nom, "size": taille, "browser_download_url": URL, "digest": "sha256:ab12"}]})
    }

    #[test]
    fn choisit_l_exe_de_la_nouvelle_version() {
        let maj = choisir(&release("v0.4.0", "stat-traque.exe", 900_000), "0.3.0").unwrap();
        let attendu =
            MiseAJour { version: "v0.4.0".into(), url: URL.into(), taille: 900_000, digest: Some("sha256:ab12".into()) };
        assert_eq!(maj, attendu);
        // Pas plus récente, pas d'exe, taille absurde, adresse étrangère : rien.
        assert!(choisir(&release("v0.3.0", "stat-traque.exe", 900_000), "0.3.0").is_none());
        assert!(choisir(&release("v0.4.0", "autre.exe", 900_000), "0.3.0").is_none());
        assert!(choisir(&release("v0.4.0", "stat-traque.exe", 10), "0.3.0").is_none());
        assert!(choisir(&json!({"message": "Not Found"}), "0.3.0").is_none());
        let mut etrangere = release("v0.4.0", "stat-traque.exe", 900_000);
        etrangere["assets"][1]["browser_download_url"] = json!("https://pirate.net/stat-traque.exe");
        assert!(choisir(&etrangere, "0.3.0").is_none());
    }

    #[test]
    fn verifie_l_empreinte_annoncee() {
        let calcule = [0xabu8; 32];
        let bon = format!("sha256:{}", "AB".repeat(32)); // majuscules acceptées
        assert!(verifier_digest(Some(&bon), &calcule).is_ok());
        assert!(verifier_digest(None, &calcule).is_ok()); // pas annoncée : autres contrôles seulement
        assert!(verifier_digest(Some(&format!("sha256:{}", "00".repeat(32))), &calcule).is_err());
        assert!(verifier_digest(Some("md5:abcd"), &calcule).is_err());
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
    fn garde_l_ancien_exe_un_lancement() {
        assert_eq!(que_faire_de_l_ancien(false, None, "0.4.0"), Ancien::Rien);
        // 1er lancement de 0.4.0 après la mise à jour : on garde, on note.
        assert_eq!(que_faire_de_l_ancien(true, None, "0.4.0"), Ancien::GarderEtNoter);
        assert_eq!(que_faire_de_l_ancien(true, Some("0.3.0"), "0.4.0"), Ancien::GarderEtNoter);
        // 0.4.0 a déjà démarré une fois : on peut supprimer.
        assert_eq!(que_faire_de_l_ancien(true, Some("0.4.0\n"), "0.4.0"), Ancien::Supprimer);
    }

    #[test]
    fn refuse_un_fichier_douteux() {
        assert!(verifier_fichier(b"MZ", 900_000, 900_000).is_ok());
        assert!(verifier_fichier(b"MZ", 899_999, 900_000).is_err());
        assert!(verifier_fichier(b"<h", 900_000, 900_000).is_err());
    }
}
