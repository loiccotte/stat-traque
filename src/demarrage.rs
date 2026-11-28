//! Lancement avec Windows : `stat-traque demarrage oui|non` (réglage `demarrage`).
//!
//! Windows lance à l'ouverture de session tout ce qui est listé dans la clé du
//! registre `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (HKCU = l'utilisateur
//! courant : pas besoin d'être administrateur). On y écrit UNE valeur :
//!   nom    = "stat-traque"
//!   donnée = "C:\chemin\vers\stat-traque.exe"   (entre guillemets, à cause des espaces)
//!
//! Coût : quelques appels Windows (advapi32, déjà présent dans tout Windows), 0 octet de RAM.
//! Hors Windows (développement) : fonctions qui ne font rien.

use std::path::Path;

/// Sous-clé du registre lue par Windows à l'ouverture de session.
#[cfg_attr(not(windows), allow(dead_code))]
const CLE_RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
/// Nom de notre valeur dans cette clé.
#[cfg_attr(not(windows), allow(dead_code))]
const NOM_VALEUR: &str = "stat-traque";

/// Texte à écrire dans le registre pour cet exe. Fonction pure.
/// Les guillemets évitent que Windows coupe le chemin au premier espace
/// ("C:\Program Files\..." serait sinon lu comme "C:\Program").
pub fn valeur_run(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Active (`true`) ou désactive (`false`) le lancement avec Windows.
pub fn activer(oui: bool) -> Result<(), String> {
    if oui {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        registre::ecrire(&valeur_run(&exe))
    } else {
        registre::effacer()
    }
}

/// Au démarrage de l'app : si `demarrage=oui`, on vérifie que le registre pointe
/// bien vers CET exe (l'utilisateur a pu déplacer le fichier). Une lecture du
/// registre (quelques microsecondes) ; une écriture seulement si ça a changé.
pub fn synchroniser(reglages: &crate::stockage::Reglages) {
    if reglages.get("demarrage") != Some("oui") {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let attendu = valeur_run(&exe);
    if registre::lire().as_deref() != Some(attendu.as_str()) {
        if let Err(e) = registre::ecrire(&attendu) {
            crate::log!("Lancement avec Windows : impossible de mettre à jour le registre ({e})");
        }
    }
}

#[cfg(windows)]
mod registre {
    use super::{CLE_RUN, NOM_VALEUR};
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    /// Texte Rust -> texte Windows (UTF-16 terminé par un zéro).
    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn ecrire(valeur: &str) -> Result<(), String> {
        let (cle, nom, donnee) = (utf16(CLE_RUN), utf16(NOM_VALEUR), utf16(valeur));
        // Taille en OCTETS, zéro final compris (u16 = 2 octets).
        let octets = (donnee.len() * 2) as u32;
        // SAFETY : toutes les chaînes sont terminées par un zéro et vivent pendant l'appel.
        let code = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, cle.as_ptr(), nom.as_ptr(), REG_SZ, donnee.as_ptr().cast(), octets)
        };
        if code == 0 { Ok(()) } else { Err(format!("erreur Windows {code}")) }
    }

    pub fn effacer() -> Result<(), String> {
        let (cle, nom) = (utf16(CLE_RUN), utf16(NOM_VALEUR));
        // SAFETY : chaînes terminées par un zéro, valides pendant l'appel.
        let code = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, cle.as_ptr(), nom.as_ptr()) };
        // Valeur déjà absente : c'est le résultat voulu.
        if code == 0 || code == ERROR_FILE_NOT_FOUND { Ok(()) } else { Err(format!("erreur Windows {code}")) }
    }

    pub fn lire() -> Option<String> {
        let (cle, nom) = (utf16(CLE_RUN), utf16(NOM_VALEUR));
        let mut tampon = [0u16; 1024];
        let mut octets = (tampon.len() * 2) as u32;
        // SAFETY : le tampon fait bien `octets` octets ; Windows y écrit au plus cette taille.
        let code = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                cle.as_ptr(),
                nom.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                tampon.as_mut_ptr().cast(),
                &mut octets,
            )
        };
        if code != 0 {
            return None;
        }
        // `octets` compte le zéro final : on l'enlève.
        let n = (octets as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&tampon[..n]))
    }
}

#[cfg(not(windows))]
mod registre {
    pub fn ecrire(_valeur: &str) -> Result<(), String> {
        Err("disponible seulement sous Windows".into())
    }
    pub fn effacer() -> Result<(), String> {
        Err("disponible seulement sous Windows".into())
    }
    pub fn lire() -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemin_entre_guillemets() {
        let exe = Path::new("C:\\Program Files\\stat-traque\\stat-traque.exe");
        assert_eq!(valeur_run(exe), "\"C:\\Program Files\\stat-traque\\stat-traque.exe\"");
    }
}
