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
//! Écrans à 125 % / 150 % : le processus est « DPI aware » (tray.rs) ; toutes les
//! tailles ci-dessous sont en pixels « à 100 % » et multipliées par l'échelle de
//! l'écran où se trouve l'overlay (police recréée si on change d'écran).
//!
//! Conformité Riot : fenêtre à part (aucune injection), rangs officiels seulement,
//! aucun timer. Invisible en « Plein écran » exclusif : mode « Sans bordure » requis.

use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, Once};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::format::{self, Cellule};
use super::palette::{self, Rgb};
use crate::affichage::VueLiveMoi;
use crate::reperes;

static FENETRE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());
static POLICE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());
/// DPI pour lequel la police a été créée (96 = 100 %).
static DPI_POLICE: AtomicU32 = AtomicU32::new(0);
/// Ce qui est affiché : grille, largeur de chaque colonne, DPI utilisé.
/// Sert aussi à ne RIEN refaire si l'état a changé sans toucher aux rangs.
static CONTENU: Mutex<Contenu> = Mutex::new(Contenu { grille: Vec::new(), largeurs: Vec::new(), dpi: 0, live: Vec::new() });
/// Petite fenêtre en jeu : `true` = état RÉDUIT (une ligne), `false` = état OUVERT (défaut).
/// Aucun raccourci secondaire n'existe encore : voir `basculer_reduit` (à brancher par tray.rs / ui/mod.rs).
static REDUIT: AtomicBool = AtomicBool::new(false);
static CLASSE: Once = Once::new();

struct Contenu {
    grille: Vec<Vec<Cellule>>,
    largeurs: Vec<i32>,
    dpi: u32,
    /// Bloc « mes chiffres en direct » affiché au-dessus du tableau (vide = absent).
    live: Vec<LigneLive>,
}

/// Une ligne de la petite fenêtre : texte à gauche, valeur alignée à droite.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LigneLive {
    gauche: String,
    droite: String,
    couleur: Rgb,
}

/// Largeur de la petite fenêtre ouverte, en pixels à 100 %.
const LARGEUR_OUVERT: i32 = 290;
/// Espace entre le bloc live et le tableau, en pixels à 100 %.
const ESPACE_BLOC: i32 = 6;

/// Nombre à une décimale avec la virgule française : 7.2 -> « 7,2 ».
fn decimale(x: f32) -> String {
    format!("{x:.1}").replace('.', ",")
}

/// Écart signé à une décimale : « +1,3 », « -0,4 », « 0,0 ».
fn ecart_decimal(ecart: f32) -> String {
    let arrondi = (ecart * 10.0).round() / 10.0;
    if arrondi > 0.0 {
        format!("+{}", decimale(arrondi))
    } else if arrondi < 0.0 {
        format!("-{}", decimale(-arrondi))
    } else {
        decimale(0.0)
    }
}

/// Écart de niveau signé : « +1 », « -2 », « = ».
fn ecart_niveau(ecart: i32) -> String {
    match ecart {
        0 => "=".into(),
        e if e > 0 => format!("+{e}"),
        e => format!("{e}"),
    }
}

/// CS par minute réels, ou `None` avant la première minute (valeur trop instable).
fn cs_par_minute(cs: u16, secondes: u32) -> Option<f32> {
    (secondes >= 60).then(|| cs as f32 * 60.0 / secondes as f32)
}

/// Entier groupé par milliers avec une espace : 5480 -> « 5 480 ».
fn milliers(n: u32) -> String {
    let chiffres = n.to_string();
    let mut sortie = String::new();
    for (i, c) in chiffres.chars().enumerate() {
        if i > 0 && (chiffres.len() - i).is_multiple_of(3) {
            sortie.push(' ');
        }
        sortie.push(c);
    }
    sortie
}

/// État RÉDUIT : « 7,2 cs/min · niveau 9 » (+ « · 5 480 or » si l'or est connu, > 0).
fn texte_reduit(v: &VueLiveMoi) -> String {
    let mut t = match cs_par_minute(v.cs, v.secondes) {
        Some(x) => format!("{} cs/min · niveau {}", decimale(x), v.niveau),
        None => format!("{} cs · niveau {}", v.cs, v.niveau),
    };
    if v.or > 0 {
        t += &format!(" · {} or", milliers(v.or));
    }
    t
}

/// État OUVERT : lignes CS/min + repère indicatif, niveau + niveau attendu, forme (si connue),
/// or (si connu). Aucune donnée adverse, aucun timer.
fn lignes_ouvertes(v: &VueLiveMoi) -> Vec<LigneLive> {
    let ligne = |g: &str, d: String, couleur| LigneLive { gauche: g.into(), droite: d, couleur };
    let mut l = Vec::new();
    let repere = reperes::cs_min_attendu(v.secondes);
    match cs_par_minute(v.cs, v.secondes) {
        Some(x) => {
            let couleur = if x >= repere { palette::VICTOIRE } else { palette::TEXTE };
            l.push(ligne("CS par minute", decimale(x), couleur));
            l.push(ligne("repère indicatif", format!("{} ({})", decimale(repere), ecart_decimal(x - repere)), palette::TEXTE_SECONDAIRE));
        }
        None => {
            l.push(ligne("CS par minute", "—".into(), palette::TEXTE));
            l.push(ligne("repère indicatif", decimale(repere), palette::TEXTE_SECONDAIRE));
        }
    }
    let attendu = reperes::niveau_attendu(v.secondes);
    l.push(ligne("Niveau", v.niveau.to_string(), palette::TEXTE));
    l.push(ligne("repère indicatif", format!("{} ({})", attendu, ecart_niveau(v.niveau as i32 - attendu as i32)), palette::TEXTE_SECONDAIRE));
    if let Some(f) = v.forme {
        l.push(ligne("Forme", format!("{f} / 100"), palette::TEXTE));
    }
    if v.or > 0 {
        l.push(ligne("Or", milliers(v.or), palette::ACCENT));
    }
    l
}

/// Lignes du bloc live selon l'état réduit / ouvert (vide si aucune donnée live).
fn lignes_live(v: Option<&VueLiveMoi>, reduit: bool) -> Vec<LigneLive> {
    match v {
        None => Vec::new(),
        Some(v) if reduit => vec![LigneLive { gauche: texte_reduit(v), droite: String::new(), couleur: palette::TEXTE }],
        Some(v) => lignes_ouvertes(v),
    }
}

/// Bascule réduit <-> ouvert sans prendre le focus (aucun clic nécessaire) et redessine.
/// Thread de l'icône. Pas encore appelée : il manque un raccourci secondaire (hors de ce module).
#[allow(dead_code)]
pub unsafe fn basculer_reduit() {
    REDUIT.fetch_xor(true, Ordering::AcqRel);
    redessiner();
}

// Tailles en pixels à 100 % (multipliées par l'échelle de l'écran).
const MARGE: i32 = 8;
const ESPACE_COLONNES: i32 = 10;
/// Espace entre l'équipe bleue (colonnes 0-2) et la rouge (3-5).
const ESPACE_EQUIPES: i32 = 18;
const HAUTEUR_LIGNE: i32 = 19;
const TAILLE_POLICE: i32 = 15;
/// Taille (diagonale) des losanges décoratifs du bord haut.
const LOSANGE: i32 = 7;
/// Distance entre le bord latéral et le centre d'un losange.
const RETRAIT_LOSANGE: i32 = 14;
/// Opacité de la fenêtre (0 = invisible, 255 = opaque).
const OPACITE: u8 = 215;

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Convertit une couleur de la palette (rouge, vert, bleu) en COLORREF GDI.
/// Attention : GDI attend l'ordre BLEU-VERT-ROUGE, soit 0x00BBGGRR (le rouge
/// dans l'octet de poids faible).
fn colorref([r, g, b]: Rgb) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

/// Les cellules de `format.rs` portent encore leurs anciennes couleurs (fichier qui
/// n'appartient pas à ce module). On les ramène ici sur la palette : une seule
/// source de couleurs à l'écran. Les couleurs de palier de rang restent telles quelles.
/// À supprimer quand `format.rs` utilisera directement `palette`.
fn couleur_palette(c: Rgb) -> Rgb {
    match c {
        [160, 155, 140] => palette::TEXTE_SECONDAIRE, // ancien GRIS
        [200, 170, 110] => palette::OR,               // ancien OR (ma ligne)
        [80, 150, 240] => palette::EQUIPE_BLEUE,
        [232, 64, 87] => palette::EQUIPE_ROUGE,
        [240, 230, 210] => palette::TEXTE, // ancien NEUTRE (Arena)
        autre => autre,
    }
}

/// Demi-diagonale d'un losange de 7 px (à 100 %) à l'échelle `dpi` : 3 px à 100 %,
/// 4 px à 125/150 %. Le losange fait donc `2 * demi + 1` px, toujours impair :
/// il a un pixel central, ses pointes tombent juste et restent nettes.
fn demi_losange(dpi: u32) -> i32 {
    (echelle(LOSANGE, dpi) / 2).max(3)
}

/// Les 4 sommets (haut, droite, bas, gauche) d'un losange centré en (`cx`, `cy`).
/// Fonction pure : testable sans fenêtre.
fn sommets_losange(cx: i32, cy: i32, demi: i32) -> [(i32, i32); 4] {
    [(cx, cy - demi), (cx + demi, cy), (cx, cy + demi), (cx - demi, cy)]
}

/// Centres (x, y) des deux losanges du bord haut d'une fenêtre de `largeur` px :
/// un près de chaque coin, posés sur le filet (le losange est entièrement visible).
fn centres_losanges(largeur: i32, dpi: u32) -> [(i32, i32); 2] {
    let demi = demi_losange(dpi);
    let retrait = echelle(RETRAIT_LOSANGE, dpi);
    [(retrait, demi), (largeur - 1 - retrait, demi)]
}

/// `valeur` (pixels à 100 %) à l'échelle d'un écran de `dpi` points par pouce.
fn echelle(valeur: i32, dpi: u32) -> i32 {
    valeur * dpi as i32 / 96
}

/// DPI de l'écran où se trouve la fenêtre (96 si inconnu).
unsafe fn dpi(fenetre: HWND) -> u32 {
    match GetDpiForWindow(fenetre) {
        0 => 96,
        d => d,
    }
}

/// Police « Segoe UI » semi-grasse pour ce DPI (recréée seulement si le DPI change).
unsafe fn police(dpi: u32) -> HFONT {
    let p = POLICE.load(Ordering::Acquire);
    if !p.is_null() && DPI_POLICE.load(Ordering::Acquire) == dpi {
        return p;
    }
    if !p.is_null() {
        DeleteObject(p);
    }
    let nom = utf16("Segoe UI");
    let hauteur = -echelle(TAILLE_POLICE, dpi);
    let f = CreateFontW(hauteur, 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, ANTIALIASED_QUALITY as u32, 0, nom.as_ptr());
    POLICE.store(f, Ordering::Release);
    DPI_POLICE.store(dpi, Ordering::Release);
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
            let mut c = CONTENU.lock().unwrap();
            c.grille.clear();
            c.live.clear();
            c.dpi = 0;
        }
        _ => {}
    }
}

/// Rectangle (x, y, largeur, hauteur) de l'écran virtuel (tous les moniteurs).
unsafe fn ecran_virtuel() -> (i32, i32, i32, i32) {
    (
        GetSystemMetrics(SM_XVIRTUALSCREEN),
        GetSystemMetrics(SM_YVIRTUALSCREEN),
        GetSystemMetrics(SM_CXVIRTUALSCREEN),
        GetSystemMetrics(SM_CYVIRTUALSCREEN),
    )
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
    // Position des réglages, ramenée sur l'écran (réglage faux, écran débranché...).
    let voulue = format::position_overlay(crate::stockage::Reglages::charger().get("overlay_position"));
    let (x, y) = format::borner_position(voulue, ecran_virtuel());
    let titre = utf16("stat-traque overlay");
    let fenetre = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
        classe.as_ptr(),
        titre.as_ptr(),
        WS_POPUP,
        x,
        y,
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

/// Relit les rangs ; si rien n'a changé à l'écran, ne fait RIEN. Sinon recalcule
/// la taille et demande un WM_PAINT. Thread de l'icône.
pub unsafe fn redessiner() {
    let fenetre = FENETRE.load(Ordering::Acquire);
    if fenetre.is_null() {
        return;
    }
    // Seulement ce qu'il faut, lu sous le verrou (pas de copie de tout l'état).
    let grille =
        crate::affichage::avec(|e| format::grille_overlay(&e.rangs, e.donnees.as_deref(), e.cle_api, e.sans_equipes));
    let live = lignes_live(crate::affichage::avec(|e| e.live_moi.clone()).as_ref(), REDUIT.load(Ordering::Acquire));
    let dpi = dpi(fenetre);
    {
        let c = CONTENU.lock().unwrap();
        if c.grille == grille && c.dpi == dpi && c.live == live {
            return; // l'état a changé, mais pas les rangs : aucun dessin
        }
    }
    // Largeur de chaque colonne = son texte le plus large.
    let dc = GetDC(fenetre);
    let ancienne = SelectObject(dc, police(dpi));
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
    // Largeur du bloc live : fixe (~290 px) à l'état ouvert, texte le plus large sinon.
    let mut largeur_live = 0;
    if !live.is_empty() {
        for l in &live {
            let t: Vec<u16> = l.gauche.encode_utf16().collect();
            let mut taille = SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut taille);
            largeur_live = largeur_live.max(taille.cx + 2 * echelle(MARGE, dpi));
        }
        if !REDUIT.load(Ordering::Acquire) {
            largeur_live = largeur_live.max(echelle(LARGEUR_OUVERT, dpi));
        }
    }
    SelectObject(dc, ancienne);
    ReleaseDC(fenetre, dc);
    let largeur_grille = 2 * echelle(MARGE, dpi) + largeurs.iter().sum::<i32>() + ecarts(colonnes, dpi);
    let largeur = largeur_grille.max(largeur_live);
    let hauteur = 2 * echelle(MARGE, dpi) + echelle(HAUTEUR_LIGNE, dpi) * grille.len() as i32 + hauteur_bloc_live(live.len(), dpi);
    *CONTENU.lock().unwrap() = Contenu { grille, largeurs, dpi, live };
    SetWindowPos(fenetre, HWND_TOPMOST, 0, 0, largeur, hauteur, SWP_NOMOVE | SWP_NOACTIVATE);
    InvalidateRect(fenetre, null_mut(), 1);
}

/// Hauteur du bloc live (lignes + espace avant le tableau), 0 s'il est vide.
fn hauteur_bloc_live(lignes: usize, dpi: u32) -> i32 {
    if lignes == 0 {
        0
    } else {
        echelle(HAUTEUR_LIGNE, dpi) * lignes as i32 + echelle(ESPACE_BLOC, dpi)
    }
}

/// Espace avant la colonne `i` (plus grand entre les deux équipes).
fn ecart_avant(i: usize, dpi: u32) -> i32 {
    echelle(if i == 3 { ESPACE_EQUIPES } else { ESPACE_COLONNES }, dpi)
}

/// Espace total entre les colonnes.
fn ecarts(colonnes: usize, dpi: u32) -> i32 {
    (1..colonnes).map(|i| ecart_avant(i, dpi)).sum()
}

/// Cadre de la fenêtre : fond de la palette, filet or de 1 px (angles droits) et
/// deux losanges or sur le bord haut. Ne dessine AUCUN contenu : réutilisable par
/// tous les états de l'overlay (réduit, ouvert...). GDI pur, aucune image.
unsafe fn dessiner_cadre(dc: HDC, zone: &RECT, dpi: u32) {
    let fond = CreateSolidBrush(colorref(palette::FOND));
    FillRect(dc, zone, fond);
    DeleteObject(fond);
    // Filet or : FrameRect trace toujours 1 pixel d'épaisseur, à toute échelle.
    let filet = CreateSolidBrush(colorref(palette::OR));
    FrameRect(dc, zone, filet);
    DeleteObject(filet);
    // Losanges : Polygon remplit avec le pinceau et contourne avec le stylo choisis.
    let pinceau = CreateSolidBrush(colorref(palette::OR));
    let stylo = CreatePen(PS_SOLID, 1, colorref(palette::OR));
    let ancien_pinceau = SelectObject(dc, pinceau);
    let ancien_stylo = SelectObject(dc, stylo);
    let demi = demi_losange(dpi);
    for (cx, cy) in centres_losanges(zone.right - zone.left, dpi) {
        let points = sommets_losange(cx, cy, demi).map(|(x, y)| POINT { x, y });
        Polygon(dc, points.as_ptr(), points.len() as i32);
    }
    SelectObject(dc, ancien_pinceau);
    SelectObject(dc, ancien_stylo);
    DeleteObject(pinceau);
    DeleteObject(stylo);
}

/// Contenu actuel : le texte de chaque case, dans la couleur de la palette.
unsafe fn dessiner_grille(dc: HDC, contenu: &Contenu, zone: &RECT) {
    let dpi = contenu.dpi.max(96);
    let ancienne = SelectObject(dc, police(dpi));
    SetBkMode(dc, TRANSPARENT as i32);
    // Bloc « mes chiffres » : libellé à gauche, valeur alignée à droite.
    for (n, l) in contenu.live.iter().enumerate() {
        let y = echelle(MARGE, dpi) + n as i32 * echelle(HAUTEUR_LIGNE, dpi);
        let gauche: Vec<u16> = l.gauche.encode_utf16().collect();
        // Ligne réduite (pas de valeur) : texte dans sa couleur ; sinon libellé en gris.
        SetTextColor(dc, colorref(if l.droite.is_empty() { l.couleur } else { palette::TEXTE_SECONDAIRE }));
        TextOutW(dc, echelle(MARGE, dpi), y, gauche.as_ptr(), gauche.len() as i32);
        if !l.droite.is_empty() {
            let droite: Vec<u16> = l.droite.encode_utf16().collect();
            SetTextColor(dc, colorref(l.couleur));
            SetTextAlign(dc, TA_RIGHT);
            TextOutW(dc, zone.right - echelle(MARGE, dpi), y, droite.as_ptr(), droite.len() as i32);
            SetTextAlign(dc, TA_LEFT);
        }
    }
    let decalage = hauteur_bloc_live(contenu.live.len(), dpi);
    for (n, ligne) in contenu.grille.iter().enumerate() {
        let mut x = echelle(MARGE, dpi);
        let y = echelle(MARGE, dpi) + decalage + n as i32 * echelle(HAUTEUR_LIGNE, dpi);
        for (i, c) in ligne.iter().enumerate() {
            if i > 0 {
                x += ecart_avant(i, dpi);
            }
            SetTextColor(dc, colorref(couleur_palette(c.couleur)));
            let t: Vec<u16> = c.texte.encode_utf16().collect();
            TextOutW(dc, x, y, t.as_ptr(), t.len() as i32);
            x += contenu.largeurs.get(i).copied().unwrap_or(0);
        }
    }
    SelectObject(dc, ancienne);
}

unsafe fn peindre(fenetre: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(fenetre, &mut ps);
    let mut zone = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(fenetre, &mut zone);
    let contenu = CONTENU.lock().unwrap();
    dessiner_cadre(dc, &zone, contenu.dpi.max(96));
    dessiner_grille(dc, &contenu, &zone);
    EndPaint(fenetre, &ps);
}

unsafe extern "system" fn procedure(fenetre: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            peindre(fenetre);
            0
        }
        // Déplacée sur un écran d'une autre échelle : nouvelle police, nouvelle taille.
        WM_DPICHANGED => {
            CONTENU.lock().unwrap().dpi = 0; // force le recalcul
            redessiner();
            0
        }
        // Les clics « traversent » : la fenêtre se déclare transparente à la souris.
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => DefWindowProcW(fenetre, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vue(secondes: u32, cs: u16, niveau: u8, or: u32, forme: Option<u8>) -> VueLiveMoi {
        VueLiveMoi { secondes, cs, niveau, or, forme }
    }

    #[test]
    fn mise_en_forme_des_nombres() {
        assert_eq!(decimale(7.2), "7,2");
        assert_eq!(ecart_decimal(1.26), "+1,3");
        assert_eq!(ecart_decimal(-0.44), "-0,4");
        assert_eq!(ecart_decimal(-0.01), "0,0");
        assert_eq!(ecart_niveau(0), "=");
        assert_eq!(ecart_niveau(2), "+2");
        assert_eq!(ecart_niveau(-1), "-1");
        assert_eq!(milliers(5480), "5 480");
        assert_eq!(milliers(480), "480");
        assert_eq!(milliers(12_345_678), "12 345 678");
    }

    #[test]
    fn ligne_reduite() {
        // 100 CS en 10 min = 10,0 ; or inconnu (0) : non affiché.
        assert_eq!(texte_reduit(&vue(600, 72, 9, 0, None)), "7,2 cs/min · niveau 9");
        assert_eq!(texte_reduit(&vue(600, 72, 9, 5480, None)), "7,2 cs/min · niveau 9 · 5 480 or");
        // Avant 1 minute : pas de cs/min instable.
        assert_eq!(texte_reduit(&vue(30, 2, 1, 0, None)), "2 cs · niveau 1");
    }

    #[test]
    fn lignes_ouvertes_forme_et_or_optionnels() {
        let sans = lignes_ouvertes(&vue(600, 72, 9, 0, None));
        assert_eq!(sans.len(), 4);
        assert_eq!(sans[0].gauche, "CS par minute");
        assert_eq!(sans[1].gauche, "repère indicatif");
        assert_eq!(sans[1].droite, "5,5 (+1,7)");
        assert!(!sans.iter().any(|l| l.gauche == "Forme" || l.gauche == "Or"));
        let avec = lignes_ouvertes(&vue(600, 72, 9, 100, Some(72)));
        assert!(avec.iter().any(|l| l.gauche == "Forme" && l.droite == "72 / 100"));
        assert!(avec.iter().any(|l| l.gauche == "Or"));
    }

    #[test]
    fn bloc_live_absent_sans_donnees() {
        assert!(lignes_live(None, false).is_empty());
        assert_eq!(hauteur_bloc_live(0, 96), 0);
        assert_eq!(lignes_live(Some(&vue(600, 72, 9, 0, None)), true).len(), 1);
    }

    #[test]
    fn colorref_ordre_bgr() {
        // Rouge pur -> octet de poids faible ; bleu pur -> octet de poids fort.
        assert_eq!(colorref([0xFF, 0, 0]), 0x0000_00FF);
        assert_eq!(colorref([0, 0xFF, 0]), 0x0000_FF00);
        assert_eq!(colorref([0, 0, 0xFF]), 0x00FF_0000);
        assert_eq!(colorref([0x12, 0x34, 0x56]), 0x0056_3412);
    }

    #[test]
    fn anciennes_couleurs_ramenees_sur_la_palette() {
        assert_eq!(couleur_palette([160, 155, 140]), palette::TEXTE_SECONDAIRE);
        assert_eq!(couleur_palette([200, 170, 110]), palette::OR);
        assert_eq!(couleur_palette([80, 150, 240]), palette::EQUIPE_BLEUE);
        assert_eq!(couleur_palette([232, 64, 87]), palette::EQUIPE_ROUGE);
        assert_eq!(couleur_palette([240, 230, 210]), palette::TEXTE);
        // Un palier de rang n'est pas touché.
        assert_eq!(couleur_palette([220, 180, 90]), [220, 180, 90]);
    }

    #[test]
    fn losange_impair_a_toutes_les_echelles() {
        assert_eq!(demi_losange(96), 3); // 7 px
        assert_eq!(demi_losange(120), 4); // 125 % : 9 px
        assert_eq!(demi_losange(144), 5); // 150 % : 10,5 -> 10/2 = 5, 11 px
    }

    #[test]
    fn sommets_symetriques() {
        let s = sommets_losange(20, 10, 3);
        assert_eq!(s, [(20, 7), (23, 10), (20, 13), (17, 10)]);
    }

    #[test]
    fn losanges_dans_la_fenetre_et_symetriques() {
        for dpi in [96, 120, 144] {
            let largeur = 300;
            let demi = demi_losange(dpi);
            let [(gx, gy), (dx, dy)] = centres_losanges(largeur, dpi);
            assert_eq!(gy, dy);
            assert_eq!(gx, largeur - 1 - dx, "symétrie gauche/droite");
            // Entièrement visibles : rien ne dépasse du haut ni des côtés.
            assert!(gy - demi >= 0 && gx - demi >= 0 && dx + demi < largeur);
        }
    }
}
