//! Overlay en jeu, version Windows NATIVE : une petite fenêtre dessinée avec GDI
//! (le dessin de texte de Windows), SANS OpenGL ni egui -> moins de 1 Mo de RAM.
//!
//! Elle vit dans le thread de l'icône (tray.rs) : c'est sa boucle de messages qui
//! la fait vivre. Les autres threads ne l'appellent jamais directement : ils
//! postent un message à l'icône (`tray::afficher_overlay`, `tray::redessiner_overlay`).
//!
//! Style de fenêtre :
//!  - WS_EX_LAYERED + SetLayeredWindowAttributes : semi-transparente ;
//!  - WS_EX_TRANSPARENT : les clics passent au jeu ;
//!  - WS_EX_TOPMOST : toujours au-dessus ;
//!  - WS_EX_NOACTIVATE : ne prend jamais le focus au jeu ;
//!  - WS_EX_TOOLWINDOW : pas de bouton dans la barre des tâches.
//!
//! Conformité Riot : fenêtre à part (aucune injection), rangs officiels seulement,
//! aucun timer. Invisible en « Plein écran » exclusif : mode « Sans bordure » requis.

use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, Once};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::format::{self, Cellule};

static FENETRE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());
static POLICE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());
/// Contenu affiché et largeur de chaque colonne (calculés hors de WM_PAINT).
static CONTENU: Mutex<(Vec<Vec<Cellule>>, Vec<i32>)> = Mutex::new((Vec::new(), Vec::new()));
static CLASSE: Once = Once::new();

const MARGE: i32 = 8;
const ESPACE_COLONNES: i32 = 10;
/// Espace entre l'équipe bleue (colonnes 0-2) et la rouge (3-5).
const ESPACE_EQUIPES: i32 = 18;
const HAUTEUR_LIGNE: i32 = 19;
/// Opacité de la fenêtre (0 = invisible, 255 = opaque).
const OPACITE: u8 = 215;

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn rgb([r, g, b]: [u8; 3]) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

/// Police « Segoe UI » semi-grasse, créée une fois et gardée.
unsafe fn police() -> HFONT {
    let p = POLICE.load(Ordering::Acquire);
    if !p.is_null() {
        return p;
    }
    let nom = utf16("Segoe UI");
    let f = CreateFontW(-15, 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, ANTIALIASED_QUALITY as u32, 0, nom.as_ptr());
    POLICE.store(f, Ordering::Release);
    f
}

/// Affiche (crée) ou masque (détruit) l'overlay. Thread de l'icône seulement.
pub unsafe fn afficher(visible: bool) {
    let fenetre = FENETRE.load(Ordering::Acquire);
    match (visible, fenetre.is_null()) {
        (true, true) => creer(),
        (false, false) => {
            DestroyWindow(fenetre);
            FENETRE.store(null_mut(), Ordering::Release);
            CONTENU.lock().unwrap().0.clear();
        }
        _ => {}
    }
}

unsafe fn creer() {
    let instance = GetModuleHandleW(null_mut());
    let classe = utf16("stat-traque-overlay");
    CLASSE.call_once(|| {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: classe.as_ptr(),
            hCursor: null_mut(),
            // SAFETY : les autres champs à zéro = valeurs par défaut.
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
    });
    let [x, y] = format::position_overlay(crate::stockage::Reglages::charger().get("overlay_position"));
    let titre = utf16("stat-traque overlay");
    let fenetre = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
        classe.as_ptr(),
        titre.as_ptr(),
        WS_POPUP,
        x as i32,
        y as i32,
        100,
        40,
        null_mut(),
        null_mut(),
        instance,
        null_mut(),
    );
    if fenetre.is_null() {
        return;
    }
    SetLayeredWindowAttributes(fenetre, 0, OPACITE, LWA_ALPHA);
    FENETRE.store(fenetre, Ordering::Release);
    redessiner();
    ShowWindow(fenetre, SW_SHOWNOACTIVATE);
}

/// Relit l'état, recalcule la taille, et demande un WM_PAINT. Thread de l'icône.
pub unsafe fn redessiner() {
    let fenetre = FENETRE.load(Ordering::Acquire);
    if fenetre.is_null() {
        return;
    }
    let etat = crate::affichage::lire(); // les gros champs sont des Arc : copie légère
    let grille = format::grille_overlay(&etat.rangs, etat.donnees.as_deref(), etat.cle_api);
    // Largeur de chaque colonne = son texte le plus large.
    let dc = GetDC(fenetre);
    let ancienne = SelectObject(dc, police());
    let colonnes = grille.iter().map(Vec::len).max().unwrap_or(0);
    let mut largeurs = vec![0i32; colonnes];
    for ligne in &grille {
        for (i, c) in ligne.iter().enumerate() {
            let t: Vec<u16> = c.texte.encode_utf16().collect();
            let mut taille = SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut taille);
            largeurs[i] = largeurs[i].max(taille.cx);
        }
    }
    SelectObject(dc, ancienne);
    ReleaseDC(fenetre, dc);
    let largeur = 2 * MARGE + largeurs.iter().sum::<i32>() + ecarts(colonnes);
    let hauteur = 2 * MARGE + HAUTEUR_LIGNE * grille.len() as i32;
    *CONTENU.lock().unwrap() = (grille, largeurs);
    SetWindowPos(fenetre, HWND_TOPMOST, 0, 0, largeur, hauteur, SWP_NOMOVE | SWP_NOACTIVATE);
    InvalidateRect(fenetre, null_mut(), 1);
}

/// Espace total entre les colonnes.
fn ecarts(colonnes: usize) -> i32 {
    (1..colonnes).map(|i| if i == 3 { ESPACE_EQUIPES } else { ESPACE_COLONNES }).sum()
}

unsafe fn peindre(fenetre: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(fenetre, &mut ps);
    let mut zone = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(fenetre, &mut zone);
    // Fond bleu nuit + bordure or sombre (couleurs du thème de la fenêtre).
    let fond = CreateSolidBrush(rgb([1, 10, 19]));
    FillRect(dc, &zone, fond);
    DeleteObject(fond);
    let bord = CreateSolidBrush(rgb([120, 90, 40]));
    FrameRect(dc, &zone, bord);
    DeleteObject(bord);

    let ancienne = SelectObject(dc, police());
    SetBkMode(dc, TRANSPARENT as i32);
    let contenu = CONTENU.lock().unwrap();
    let (grille, largeurs) = &*contenu;
    for (n, ligne) in grille.iter().enumerate() {
        let mut x = MARGE;
        let y = MARGE + n as i32 * HAUTEUR_LIGNE;
        for (i, c) in ligne.iter().enumerate() {
            if i > 0 {
                x += if i == 3 { ESPACE_EQUIPES } else { ESPACE_COLONNES };
            }
            SetTextColor(dc, rgb(c.couleur));
            let t: Vec<u16> = c.texte.encode_utf16().collect();
            TextOutW(dc, x, y, t.as_ptr(), t.len() as i32);
            x += largeurs.get(i).copied().unwrap_or(0);
        }
    }
    SelectObject(dc, ancienne);
    EndPaint(fenetre, &ps);
}

unsafe extern "system" fn procedure(fenetre: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            peindre(fenetre);
            0
        }
        // Les clics « traversent » : la fenêtre se déclare transparente à la souris.
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => DefWindowProcW(fenetre, msg, wparam, lparam),
    }
}
