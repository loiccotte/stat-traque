//! Icônes de champions (Data Dragon) : chemin du cache, URL, décodage PNG, réduction.
//!
//! Stub : les signatures sont le contrat avec les autres modules ; les corps sont
//! remplis par T-02. Rien n'est téléchargé ni décodé tant que ce n'est pas fait.

// Rempli et branché par T-02 : en attendant, rien n'appelle ces fonctions.
#![allow(dead_code)]

use std::path::PathBuf;

/// Taille voulue d'une icône : petite (ligne de tableau) ou grande (fiche).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taille {
    Ligne,
    Fiche,
}

/// Icône décodée : carré de `cote` pixels, RGBA 8 bits (`cote * cote * 4` octets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icone {
    pub cote: u16,
    pub rgba: Vec<u8>,
}

/// L'identifiant Data Dragon (ex. « Ahri ») est-il sûr à mettre dans un chemin / une URL ?
pub fn id_dd_valide(_id: &str) -> bool {
    false // T-02
}

/// Fichier de cache de l'icône dans `%APPDATA%\stat-traque\cache\`.
pub fn chemin(_version: &str, _id_dd: &str) -> PathBuf {
    PathBuf::new() // T-02
}

/// URL Data Dragon de l'icône.
pub fn url(_version: &str, _id_dd: &str) -> String {
    String::new() // T-02
}

/// Décode un PNG et le réduit à la taille demandée.
pub fn decoder(_png: &[u8], _t: Taille) -> Option<Icone> {
    None // T-02
}

/// Réduit une image RGBA carrée de `src` pixels de côté vers `dst` pixels.
pub fn reduire(_rgba: &[u8], _src: u32, _dst: u32) -> Vec<u8> {
    Vec::new() // T-02
}

/// Charge une icône depuis le cache disque (jamais de téléchargement ici).
pub fn charger(_version: &str, _id_dd: &str, _t: Taille) -> Option<Icone> {
    None // T-02
}

/// Supprime du cache les icônes des anciens patchs.
pub fn menage(_version_courante: &str) {
    // T-02
}
