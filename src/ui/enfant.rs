//! Le PROCESSUS de la fenêtre (`stat-traque --fenetre`), lancé par le processus
//! principal (parent.rs) à l'ouverture, et qui se termine à la fermeture : tout ce
//! qu'il a chargé (pilote OpenGL, polices, textures) disparaît avec lui.
//!
//! Il ne parle ni au client LoL ni à l'API Riot : il lit l'état d'affichage, une
//! ligne JSON par changement, sur son entrée standard (voir protocole.rs).
//! Entrée standard fermée = le processus principal demande la fermeture.
//! Ses DEMANDES (fiche d'un joueur...) partent sur sa sortie standard (`demander`).
//!
//! Rendu seulement quand il le faut : à chaque message reçu (`request_repaint`)
//! ou action de la souris / du clavier. Jamais de boucle à 60 images/s.

use std::io::{BufRead, Write};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use eframe::egui;

use super::ecrans::{self, Ecran};
use super::protocole::{appliquer_message, ligne_demande, lire_message, meme_version, Demande, Message};
use super::{demo, format, overlay, theme};
use crate::affichage::EtatAffichage;

/// Ce que le thread de lecture partage avec la fenêtre.
#[derive(Default)]
struct Partage {
    etat: EtatAffichage,
    /// +1 à chaque message : la fenêtre ne recopie l'état que s'il a changé.
    version: u64,
    premier_plan: bool,
    /// Entrée fermée : il faut fermer la fenêtre.
    fermer: bool,
    /// Le processus de l'icône n'a pas la même version que nous (mise à jour
    /// installée pendant qu'il tournait) : on n'affiche qu'un message.
    autre_version: bool,
}

static PARTAGE: Mutex<Option<Partage>> = Mutex::new(None);
/// Contexte egui, pour demander un redessin depuis le thread de lecture.
static CONTEXTE: OnceLock<egui::Context> = OnceLock::new();

fn partage<R>(f: impl FnOnce(&mut Partage) -> R) -> R {
    f(PARTAGE.lock().unwrap().get_or_insert_with(Partage::default))
}

fn redessiner() {
    if let Some(ctx) = CONTEXTE.get() {
        ctx.request_repaint();
    }
}

/// Canal vers le thread « demandes » de CE processus (créé au 1er envoi).
static DEMANDES: OnceLock<Mutex<mpsc::Sender<String>>> = OnceLock::new();

/// Envoie une demande au processus principal (canal retour : une ligne JSON sur
/// notre sortie standard, lue par parent.rs). Ne bloque jamais la fenêtre :
/// l'écriture se fait dans un petit thread à part.
pub fn demander(d: &Demande) {
    let tx = DEMANDES.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<String>();
        let _ = std::thread::Builder::new().name("demandes".into()).spawn(move || {
            for l in rx {
                let mut sortie = std::io::stdout().lock();
                // Erreur = le processus principal ne lit plus : rien à faire.
                if writeln!(sortie, "{l}").and_then(|_| sortie.flush()).is_err() {
                    break;
                }
            }
        });
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap().send(ligne_demande(d));
}

/// Copie un texte dans le presse-papiers. Renvoie false si ça a échoué.
///
/// eframe est compilé SANS presse-papiers (dépendance `arboard` évitée) : sous
/// Windows on appelle directement l'API du système (quelques fonctions, coût ~0).
/// `ctx.copy_text` garde aussi le texte pour egui (Ctrl+V dans un champ de l'app ;
/// sous Linux, c'est la seule copie : suffisant pour le développement).
pub fn copier(ctx: &egui::Context, texte: &str) -> bool {
    ctx.copy_text(texte.to_string());
    #[cfg(windows)]
    {
        presse_papiers_windows(texte)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(windows)]
fn presse_papiers_windows(texte: &str) -> bool {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    /// Format « texte Unicode » (UTF-16) du presse-papiers.
    const CF_UNICODETEXT: u32 = 13;
    // Windows veut de l'UTF-16 terminé par un zéro, avec des fins de ligne \r\n.
    let utf16: Vec<u16> = texte.replace("\r\n", "\n").replace('\n', "\r\n").encode_utf16().chain([0]).collect();
    // SAFETY : suite d'appels documentée (OpenClipboard -> EmptyClipboard ->
    // SetClipboardData -> CloseClipboard). Après un SetClipboardData réussi, la
    // mémoire appartient à Windows : on ne la libère que si ça a échoué.
    unsafe {
        // Un autre programme peut tenir le presse-papiers un court instant : 5 essais.
        // Propriétaire : notre fenêtre (au premier plan, on vient de cliquer dessus).
        let fenetre = GetForegroundWindow();
        let mut ouvert = false;
        for _ in 0..5 {
            if OpenClipboard(fenetre) != 0 {
                ouvert = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !ouvert {
            return false;
        }
        let mut ok = false;
        let memoire = GlobalAlloc(GMEM_MOVEABLE, utf16.len() * 2);
        if !memoire.is_null() {
            let p = GlobalLock(memoire) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(utf16.as_ptr(), p, utf16.len());
                GlobalUnlock(memoire);
                ok = EmptyClipboard() != 0 && !SetClipboardData(CF_UNICODETEXT, memoire).is_null();
            }
            if !ok {
                GlobalFree(memoire);
            }
        }
        CloseClipboard();
        ok
    }
}

/// Point d'entrée de `stat-traque --fenetre [overlay]`. Revient à la fermeture.
pub fn lancer(overlay: bool) {
    // 1. Lecture de l'entrée standard, dans un thread (bloquant, 0 % CPU en attente).
    std::thread::Builder::new()
        .name("lecture".into())
        .spawn(|| {
            let entree = std::io::stdin();
            let mut lignes = entree.lock().lines();
            // 1re ligne : la version du processus de l'icône.
            let version_ok = lignes.next().and_then(Result::ok).is_some_and(|l| meme_version(&l));
            if !version_ok {
                partage(|p| p.autre_version = true);
                redessiner();
            }
            for ligne in lignes {
                let Ok(ligne) = ligne else { break };
                if !version_ok {
                    continue; // format peut-être différent : on ne lit rien
                }
                // Lecture du JSON (le plus long) HORS du verrou : la fenêtre n'attend pas.
                let Some(message) = lire_message(&ligne) else { continue };
                partage(|p| {
                    match appliquer_message(&mut p.etat, message) {
                        Some(Message::PremierPlan) => p.premier_plan = true,
                        Some(Message::Demo) => p.etat.donnees = Some(Arc::new(demo::donnees())),
                        _ => {}
                    }
                    p.version += 1;
                });
                redessiner();
            }
            // Fin de l'entrée : le processus principal veut qu'on se ferme (ou il a disparu).
            partage(|p| p.fermer = true);
            redessiner();
            if CONTEXTE.get().is_none() {
                std::process::exit(0); // fenêtre pas encore créée : on part tout de suite
            }
        })
        .expect("thread de lecture");

    // 2. Noms des champions / objets : depuis le cache Data Dragon (pas d'internet).
    std::thread::spawn(|| {
        if let Some(d) = crate::ddragon::charger_cache() {
            let d = Arc::new(d);
            partage(|p| {
                if p.etat.donnees.is_none() {
                    p.etat.donnees = Some(d);
                    p.version += 1;
                }
            });
            redessiner();
        }
    });

    // 3. La fenêtre (ici, dans le thread principal de CE processus).
    let viewport = if overlay {
        // Linux (développement) seulement : sous Windows l'overlay est natif (tray.rs).
        let position = format::position_overlay(crate::stockage::Reglages::charger().get("overlay_position"));
        egui::ViewportBuilder::default()
            .with_title("stat-traque overlay")
            .with_inner_size(overlay::TAILLE)
            .with_position(position)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_taskbar(false)
            .with_active(false)
            .with_resizable(false)
    } else {
        egui::ViewportBuilder::default()
            .with_title("stat-traque")
            .with_inner_size([960.0, 680.0])
            .with_min_inner_size([640.0, 420.0])
    };
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Glow, ..Default::default() };
    let resultat = eframe::run_native(
        "stat-traque",
        options,
        Box::new(move |cc| {
            theme::appliquer(&cc.egui_ctx);
            let _ = CONTEXTE.set(cc.egui_ctx.clone());
            Ok(Box::new(Appli::nouvelle(overlay)))
        }),
    );
    if let Err(e) = resultat {
        eprintln!("Fenêtre impossible à ouvrir : {e}");
    }
}

struct Appli {
    overlay: bool,
    /// Copie locale de l'état (les gros champs sont des `Arc` : copie = un pointeur).
    etat: EtatAffichage,
    version: u64,
    ecran: Ecran,
}

impl Appli {
    fn nouvelle(overlay: bool) -> Self {
        let (etat, version) = partage(|p| (p.etat.clone(), p.version));
        let ecran = Ecran::nouveau(&etat);
        Self { overlay, etat, version, ecran }
    }
}

impl eframe::App for Appli {
    /// Appelée avant chaque image ET quand la fenêtre est réduite (dans ce cas
    /// `ui` ne l'est pas) : c'est donc ici qu'on traite fermeture et premier plan.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (fermer, premier_plan) = partage(|p| (p.fermer, std::mem::take(&mut p.premier_plan)));
        if fermer {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if premier_plan {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if partage(|p| p.autre_version) {
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::FOND)).show(ui, |ui| {
                ui.add_space(80.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Mise à jour installée : redémarre stat-traque").color(theme::OR).size(20.0));
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Clic droit sur l'icône près de l'horloge, Quitter, puis relance stat-traque.")
                            .color(theme::TEXTE_DOUX),
                    );
                });
            });
            return;
        }
        // Copie de l'état seulement s'il a changé.
        if let Some((etat, version)) = partage(|p| (p.version != self.version).then(|| (p.etat.clone(), p.version))) {
            self.etat = etat;
            self.version = version;
            self.ecran.etat_change(&self.etat);
        }
        if self.overlay {
            overlay::dessiner(ui, &self.etat);
        } else {
            ecrans::dessiner(ui, &mut self.ecran, &self.etat);
        }
    }

    /// Couleur de fond : opaque pour la fenêtre, transparente pour l'overlay.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.overlay { [0.0; 4] } else { theme::FOND.to_normalized_gamma_f32() }
    }
}
