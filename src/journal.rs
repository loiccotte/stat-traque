//! Journal : affiche les messages dans la console ET les écrit dans un fichier
//! (%APPDATA%\stat-traque\journal.txt). En mode tray il n'y a pas de console,
//! le fichier permet donc de savoir ce qui s'est passé.

use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

/// Le fichier est ouvert une seule fois puis partagé entre les threads (Mutex).
static FICHIER: Mutex<Option<File>> = Mutex::new(None);

/// Crée (ou vide) le fichier journal. À appeler une fois au démarrage.
pub fn initialiser() {
    let chemin = crate::stockage::dossier_app().join("journal.txt");
    if let Ok(f) = File::create(chemin) {
        *FICHIER.lock().unwrap() = Some(f);
    }
}

/// Écrit une ligne. Utilisée via la macro `log!` (comme `println!`).
pub fn ecrire(message: std::fmt::Arguments) {
    println!("{message}");
    if let Some(f) = FICHIER.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{message}");
    }
}

/// `log!("Bonjour {}", nom)` : s'utilise exactement comme `println!`.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::journal::ecrire(format_args!($($arg)*)) };
}
