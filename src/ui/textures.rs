//! Textures des portraits de champions : au plus ~10 en RAM, libérées en fin de trame.
//!
//! Stub : `texture` renvoie toujours `None` (le portrait sera alors un carré vide).
//! Rempli par T-03.

// Rempli et branché par T-03 : en attendant, rien n'appelle ces fonctions.
#![allow(dead_code)]

use crate::icones::Taille;

/// Cache de textures de portraits, propre à la fenêtre (détruit avec elle).
pub struct Portraits {
    _version: Option<String>,
}

impl Portraits {
    /// `version` : patch Data Dragon courant (None tant qu'on ne le connaît pas).
    pub fn nouveau(_ctx: &eframe::egui::Context, version: Option<String>) -> Self {
        Self { _version: version }
    }

    /// Texture du portrait de `id_dd`, ou `None` s'il n'est pas (encore) disponible.
    pub fn texture(&mut self, _id_dd: &str, _t: Taille) -> Option<eframe::egui::TextureId> {
        None // T-03
    }

    /// À appeler une fois par trame : libère les textures non utilisées.
    pub fn fin_de_trame(&mut self) {
        // T-03
    }
}
