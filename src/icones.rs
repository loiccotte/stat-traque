//! Icônes de champions (Data Dragon) : chemin du cache, URL, décodage PNG, réduction.
//!
//! Principe : le PNG (~27 Ko, 120 px) est téléchargé UNE fois par patch, gardé tel quel
//! sur le disque, puis décodé à la demande et réduit (48 px ou 96 px). Seul le résultat
//! réduit reste en RAM (l'appelant le met dans un cache LRU borné) ; le PNG d'origine
//! et l'image 120 px sont libérés dès la fin du décodage.
//!
//! RAM transitoire d'un décodage (calcul, pour une icône Data Dragon 120x120) :
//!   - PNG lu en mémoire ............ ~27 Ko
//!   - image décodée 120x120 RGBA .... 57,6 Ko (120*120*4)
//!   - tampon interne de `png` ....... quelques Ko (une ligne + fenêtre de décompression)
//!   - résultat 48x48 RGBA ........... 9,2 Ko (96x96 : 36,9 Ko)
//!
//!   => ~60 Ko d'image à la pointe (~100 Ko en comptant le PNG), tout libéré ensuite.
//!   Résident par icône conservée : 9,2 Ko (Ligne) ou 36,9 Ko (Fiche).
//!   Plafonds en dur : entrée <= 200 Ko, côté <= 256 px => au plus 256 Ko de pixels.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{http, stockage};

/// Taille maximale acceptée pour un PNG (téléchargé ou lu sur le disque).
const MAX_OCTETS: usize = 200 * 1024;
/// Côté maximal accepté pour l'image source, en pixels.
const MAX_COTE: u32 = 256;
/// Délai court pour le téléchargement d'une icône.
const DELAI: Duration = Duration::from_secs(4);
/// Nom du sous-dossier de cache.
const DOSSIER: &str = "portraits";

/// Taille voulue d'une icône : petite (ligne de tableau) ou grande (fiche).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taille {
    Ligne,
    Fiche,
}

impl Taille {
    /// Côté en pixels de l'icône réduite.
    fn pixels(self) -> u32 {
        match self {
            Taille::Ligne => 48,
            Taille::Fiche => 96,
        }
    }
}

/// Icône décodée : carré de `cote` pixels, RGBA 8 bits (`cote * cote * 4` octets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icone {
    pub cote: u16,
    pub rgba: Vec<u8>,
}

/// L'identifiant Data Dragon (ex. « Ahri ») est-il sûr à mettre dans un chemin / une URL ?
/// Seulement des lettres/chiffres ASCII, de 1 à 32 caractères.
pub fn id_dd_valide(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Une version de patch (ex. « 14.23.1 ») est-elle sûre pour un nom de dossier ?
/// Chiffres, lettres, point, tiret ; jamais de séparateur ni de « .. ».
fn version_valide(v: &str) -> bool {
    (1..=32).contains(&v.len())
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && !v.contains("..")
        && !v.starts_with('.')
}

/// Fichier de cache de l'icône dans `%APPDATA%\stat-traque\cache\portraits\<version>\<Id>.png`.
/// Si la version ou l'id est suspect, renvoie un chemin vide (qui ne désigne rien d'utile).
pub fn chemin(version: &str, id_dd: &str) -> PathBuf {
    if !version_valide(version) || !id_dd_valide(id_dd) {
        return PathBuf::new();
    }
    stockage::dossier_cache(DOSSIER)
        .join(version)
        .join(format!("{id_dd}.png"))
}

/// URL Data Dragon de l'icône.
pub fn url(version: &str, id_dd: &str) -> String {
    format!("https://ddragon.leagueoflegends.com/cdn/{version}/img/champion/{id_dd}.png")
}

/// Décode un PNG et le réduit à la taille demandée. `None` si le PNG est invalide,
/// trop gros (> 200 Ko), trop grand (> 256 px de côté) ou pas carré.
pub fn decoder(png: &[u8], t: Taille) -> Option<Icone> {
    if png.is_empty() || png.len() > MAX_OCTETS {
        return None;
    }
    let mut dec = png::Decoder::new(Cursor::new(png));
    // Ramène tout en 8 bits : palette -> couleurs, 16 bits -> 8 bits, gris < 8 bits -> 8 bits.
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut lecteur = dec.read_info().ok()?;

    // On vérifie les dimensions AVANT d'allouer le tampon de pixels.
    let (l, h) = {
        let i = lecteur.info();
        (i.width, i.height)
    };
    if l == 0 || l != h || l > MAX_COTE {
        return None;
    }
    let mut tampon = vec![0u8; lecteur.output_buffer_size()?];
    let image = lecteur.next_frame(&mut tampon).ok()?;
    let pixels = &tampon[..image.buffer_size()];
    let n = (l * h) as usize;

    // Conversion vers RGBA 8 bits, quel que soit le format source.
    let rgba: Vec<u8> = match image.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        // Ne devrait pas arriver (EXPAND convertit la palette) : on refuse par prudence.
        png::ColorType::Indexed => return None,
    };
    if rgba.len() != n * 4 {
        return None;
    }

    let dst = t.pixels();
    if dst >= l {
        // Jamais d'agrandissement : on garde la taille d'origine.
        return Some(Icone {
            cote: l as u16,
            rgba,
        });
    }
    Some(Icone {
        cote: dst as u16,
        rgba: reduire(&rgba, l, dst),
    })
}

/// Réduit une image RGBA carrée de `src` pixels de côté vers `dst` pixels
/// (filtre en boîte : chaque pixel de sortie = moyenne du bloc source qui lui correspond).
/// La moyenne est pondérée par l'alpha pour ne pas faire baver les pixels transparents.
/// Si les paramètres sont incohérents (taille du tampon, dst > src, zéro), renvoie un
/// tampon vide plutôt que de paniquer.
pub fn reduire(rgba: &[u8], src: u32, dst: u32) -> Vec<u8> {
    let (s, d) = (src as usize, dst as usize);
    if s == 0 || d == 0 || d > s || rgba.len() != s * s * 4 {
        return Vec::new();
    }
    let mut sortie = Vec::with_capacity(d * d * 4);
    for dy in 0..d {
        let (y0, y1) = bornes(dy, d, s);
        for dx in 0..d {
            let (x0, x1) = bornes(dx, d, s);
            // Sommes : R, G, B pondérés par l'alpha, puis alpha seul.
            let (mut r, mut g, mut b, mut a) = (0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * s + x) * 4;
                    let al = rgba[i + 3] as u64;
                    r += rgba[i] as u64 * al;
                    g += rgba[i + 1] as u64 * al;
                    b += rgba[i + 2] as u64 * al;
                    a += al;
                }
            }
            let nb = ((y1 - y0) * (x1 - x0)) as u64;
            // Arrondi au plus proche ; si tout le bloc est transparent (a == 0), la
            // division est impossible : on écrit un pixel transparent.
            match (
                (r + a / 2).checked_div(a),
                (g + a / 2).checked_div(a),
                (b + a / 2).checked_div(a),
            ) {
                (Some(r), Some(g), Some(b)) => {
                    sortie.push(r as u8);
                    sortie.push(g as u8);
                    sortie.push(b as u8);
                    sortie.push(((a + nb / 2) / nb) as u8);
                }
                _ => sortie.extend_from_slice(&[0, 0, 0, 0]),
            }
        }
    }
    sortie
}

/// Intervalle source [début, fin[ couvert par le pixel `i` de sortie (au moins 1 pixel).
fn bornes(i: usize, dst: usize, src: usize) -> (usize, usize) {
    let debut = i * src / dst;
    let fin = ((i + 1) * src / dst).max(debut + 1);
    (debut, fin.min(src))
}

/// Charge une icône : lit le cache disque ; sinon la télécharge (une seule requête,
/// 200 Ko max, délai court), l'écrit dans le cache puis la décode.
/// BLOQUANT : l'appelant doit l'exécuter dans un thread, jamais dans la boucle d'affichage.
/// Ne panique jamais ; `None` en cas d'échec (réseau, PNG invalide, id suspect...).
pub fn charger(version: &str, id_dd: &str, t: Taille) -> Option<Icone> {
    if !version_valide(version) || !id_dd_valide(id_dd) {
        return None;
    }
    let fichier = chemin(version, id_dd);

    // 1. Cache disque (lecture plafonnée : un fichier géant est ignoré).
    if let Some(octets) = lire_plafonne(&fichier) {
        if let Some(icone) = decoder(&octets, t) {
            return Some(icone);
        }
        // Fichier corrompu : on le supprime pour le retélécharger.
        let _ = std::fs::remove_file(&fichier);
    }

    // 2. Téléchargement.
    let reponse = http::internet()
        .get(&url(version, id_dd))
        .timeout(DELAI)
        .call()
        .ok()?;
    let mut octets = Vec::with_capacity(32 * 1024);
    // On lit au plus MAX_OCTETS + 1 : si on dépasse, la réponse est refusée.
    reponse
        .into_reader()
        .take(MAX_OCTETS as u64 + 1)
        .read_to_end(&mut octets)
        .ok()?;
    // On décode d'abord : on ne met jamais un fichier invalide dans le cache.
    let icone = decoder(&octets, t)?;

    // 3. Écriture atomique : fichier .tmp puis renommage (jamais de PNG à moitié écrit).
    // Un échec d'écriture n'empêche pas d'afficher l'icône.
    let _ = ecrire_atomique(&fichier, &octets);
    Some(icone)
}

/// Lit un fichier de cache s'il fait au plus `MAX_OCTETS`.
fn lire_plafonne(fichier: &Path) -> Option<Vec<u8>> {
    let f = std::fs::File::open(fichier).ok()?;
    let mut octets = Vec::new();
    f.take(MAX_OCTETS as u64 + 1)
        .read_to_end(&mut octets)
        .ok()?;
    (octets.len() <= MAX_OCTETS).then_some(octets)
}

/// Écrit `octets` dans `fichier` via un fichier temporaire voisin puis un renommage.
fn ecrire_atomique(fichier: &Path, octets: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = fichier.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = fichier.with_extension("png.tmp");
    std::fs::write(&tmp, octets)?;
    let res = std::fs::rename(&tmp, fichier);
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Supprime du cache les icônes des anciens patchs : tous les DOSSIERS de
/// `cache\portraits\` sauf celui de la version courante. Jamais autre chose
/// (les fichiers et les liens symboliques sont laissés intacts).
pub fn menage(version_courante: &str) {
    if !version_valide(version_courante) {
        return; // Dans le doute, on ne supprime rien.
    }
    let Ok(entrees) = std::fs::read_dir(stockage::dossier_cache(DOSSIER)) else {
        return;
    };
    for e in entrees.flatten() {
        // `file_type()` ne suit pas les liens : un lien n'est jamais pris pour un dossier.
        let est_dossier = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if est_dossier && e.file_name() != std::ffi::OsStr::new(version_courante) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode un PNG en mémoire (le test n'a besoin d'aucun fichier externe).
    fn png_de(
        l: u32,
        h: u32,
        couleur: png::ColorType,
        profondeur: png::BitDepth,
        data: &[u8],
    ) -> Vec<u8> {
        let mut sortie = Vec::new();
        let mut enc = png::Encoder::new(&mut sortie, l, h);
        enc.set_color(couleur);
        enc.set_depth(profondeur);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(data).unwrap();
        w.finish().unwrap();
        sortie
    }

    #[test]
    fn id_valides_et_invalides() {
        for ok in ["Ahri", "LeeSin", "Kaisa", "A1"] {
            assert!(id_dd_valide(ok), "{ok}");
        }
        assert!(!id_dd_valide(""));
        assert!(!id_dd_valide("../x"));
        assert!(!id_dd_valide("a b"));
        assert!(!id_dd_valide("a/b"));
        assert!(!id_dd_valide("Nunu&Willump"));
        assert!(id_dd_valide(&"a".repeat(32)));
        assert!(!id_dd_valide(&"a".repeat(33)));
    }

    #[test]
    fn reduire_carre_uni() {
        let src = [10u8, 20, 30, 255].repeat(16); // 4x4
        let r = reduire(&src, 4, 2);
        assert_eq!(r, [10u8, 20, 30, 255].repeat(4));
        // Tailles non divisibles (5 -> 2) : toujours uni.
        let src = [7u8, 8, 9, 255].repeat(25);
        assert_eq!(reduire(&src, 5, 2), [7u8, 8, 9, 255].repeat(4));
    }

    #[test]
    fn reduire_damier() {
        // Damier noir/blanc 4x4 : chaque bloc 2x2 contient 2 noirs et 2 blancs -> gris 128.
        let mut src = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                let v = if (x + y) % 2 == 0 { 0 } else { 255 };
                src.extend_from_slice(&[v, v, v, 255]);
            }
        }
        assert_eq!(reduire(&src, 4, 2), [128u8, 128, 128, 255].repeat(4));
    }

    #[test]
    fn reduire_parametres_incoherents() {
        assert!(reduire(&[0; 10], 4, 2).is_empty());
        assert!(reduire(&[0; 64], 4, 8).is_empty());
        assert!(reduire(&[], 0, 0).is_empty());
    }

    #[test]
    fn reduire_transparent_ne_bave_pas() {
        // Un pixel rouge opaque + 3 transparents : la couleur reste rouge, alpha = 64.
        let mut src = vec![0u8; 16];
        src[..4].copy_from_slice(&[255, 0, 0, 255]);
        assert_eq!(reduire(&src, 2, 1), vec![255, 0, 0, 64]);
    }

    #[test]
    fn decoder_png_valide() {
        // 8x8 RGB uni (vert) -> pas de réduction car 8 < 48 : taille conservée.
        let data = [0u8, 200, 0].repeat(64);
        let png = png_de(8, 8, png::ColorType::Rgb, png::BitDepth::Eight, &data);
        let i = decoder(&png, Taille::Ligne).expect("PNG valide");
        assert_eq!(i.cote, 8);
        assert_eq!(i.rgba, [0u8, 200, 0, 255].repeat(64));
    }

    #[test]
    fn decoder_reduit_a_48_et_96() {
        let data = [1u8, 2, 3, 255].repeat(120 * 120);
        let png = png_de(120, 120, png::ColorType::Rgba, png::BitDepth::Eight, &data);
        let a = decoder(&png, Taille::Ligne).unwrap();
        assert_eq!((a.cote, a.rgba.len()), (48, 48 * 48 * 4));
        let b = decoder(&png, Taille::Fiche).unwrap();
        assert_eq!((b.cote, b.rgba.len()), (96, 96 * 96 * 4));
        assert_eq!(&a.rgba[..4], &[1, 2, 3, 255]);
    }

    #[test]
    fn decoder_autres_formats() {
        // Gris 8 bits.
        let png = png_de(
            2,
            2,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            &[9, 9, 9, 9],
        );
        assert_eq!(
            decoder(&png, Taille::Ligne).unwrap().rgba,
            [9u8, 9, 9, 255].repeat(4)
        );
        // Gris + alpha.
        let png = png_de(
            1,
            1,
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            &[50, 100],
        );
        assert_eq!(
            decoder(&png, Taille::Ligne).unwrap().rgba,
            vec![50, 50, 50, 100]
        );
        // RGB 16 bits : 0xFF00 -> 255 (8 bits de poids fort).
        let png = png_de(
            1,
            1,
            png::ColorType::Rgb,
            png::BitDepth::Sixteen,
            &[0xFF, 0, 0x80, 0, 0, 0],
        );
        assert_eq!(
            decoder(&png, Taille::Ligne).unwrap().rgba,
            vec![255, 128, 0, 255]
        );
    }

    #[test]
    fn decoder_refuse_hors_limites() {
        // Trop grand : 300 px de côté.
        let data = vec![0u8; 300 * 300];
        let png = png_de(
            300,
            300,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            &data,
        );
        assert!(decoder(&png, Taille::Ligne).is_none());
        // Pas carré.
        let png = png_de(
            4,
            2,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            &[0; 8],
        );
        assert!(decoder(&png, Taille::Ligne).is_none());
        // Trop lourd en octets (> 200 Ko).
        assert!(decoder(&vec![0u8; MAX_OCTETS + 1], Taille::Ligne).is_none());
        // Déchets / vide.
        assert!(decoder(b"pas un png", Taille::Ligne).is_none());
        assert!(decoder(&[], Taille::Ligne).is_none());
    }

    #[test]
    fn chemin_refuse_les_versions_piegees() {
        for v in [
            "",
            "..",
            "../x",
            "a/b",
            "a\\b",
            "14.1/../..",
            ".cache",
            "1 2",
            "C:",
        ] {
            assert_eq!(chemin(v, "Ahri"), PathBuf::new(), "version {v:?}");
        }
        assert_eq!(chemin("14.1.1", "../Ahri"), PathBuf::new());
        let c = chemin("14.23.1", "Ahri");
        assert!(c.ends_with(Path::new("portraits").join("14.23.1").join("Ahri.png")));
    }

    #[test]
    fn url_correcte() {
        assert_eq!(
            url("14.23.1", "LeeSin"),
            "https://ddragon.leagueoflegends.com/cdn/14.23.1/img/champion/LeeSin.png"
        );
    }

    #[test]
    fn charger_refuse_les_entrees_suspectes() {
        // Aucun accès réseau : refus immédiat.
        assert!(charger("../x", "Ahri", Taille::Ligne).is_none());
        assert!(charger("14.1.1", "a/b", Taille::Ligne).is_none());
    }
}
