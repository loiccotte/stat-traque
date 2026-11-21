//! La fenêtre et l'overlay : création à la demande, destruction à la fermeture.
//!
//! ARCHITECTURE (à lire avant de modifier) :
//!  - Le thread principal est pris par l'icône (tray.rs). La fenêtre tourne donc
//!    dans UN thread dédié, « fenetre », créé une seule fois au démarrage.
//!  - Ce thread DORT (Condvar, 0 % CPU) tant qu'aucune fenêtre n'est demandée.
//!  - Quand on demande la fenêtre (clic sur l'icône...), il appelle
//!    `eframe::run_native` qui crée la fenêtre + le contexte OpenGL et ne REVIENT
//!    qu'à la fermeture : fenêtre, contexte OpenGL, textures et polices sont alors
//!    détruits -> la RAM est rendue. Puis le thread se rendort.
//!  - winit interdit de créer deux « boucles d'événements » dans un processus :
//!    eframe garde la sienne dans le thread (mode `run_and_return`), c'est pour
//!    ça que c'est TOUJOURS le même thread qui ouvre les fenêtres.
//!  - Une seule fenêtre à la fois : la fenêtre principale OU l'overlay. Ouvrir
//!    l'une ferme l'autre (moins de RAM en partie, et pas de fenêtre fantôme).
//!  - Pas de boucle à 60 images/s : egui ne redessine que sur un événement
//!    (souris, clavier) ou quand le cerveau change l'état (`quand_ca_change`).

use std::sync::{Condvar, Mutex, Once};

use eframe::egui;

use super::ecrans::{self, Ecran};
use super::{format, overlay, theme};
use crate::affichage::{self, EtatAffichage};
use crate::gameflow::Phase;

/// Quelle fenêtre ce passage de `run_native` affiche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Racine {
    Principale,
    Overlay,
}

/// Ce qui est DEMANDÉ (par l'icône, le raccourci, le cerveau) ; le thread
/// « fenetre » s'y conforme.
struct Demandes {
    fenetre: bool,
    overlay: bool,
    /// Ramener la fenêtre principale au premier plan (déjà ouverte).
    premier_plan: bool,
    /// Ce qui tourne réellement en ce moment.
    en_cours: Option<Racine>,
    /// Dernière phase vue (pour réagir aux CHANGEMENTS de phase seulement).
    phase: Option<Phase>,
}

static DEMANDES: Mutex<Demandes> =
    Mutex::new(Demandes { fenetre: false, overlay: false, premier_plan: false, en_cours: None, phase: None });
/// Réveille le thread « fenetre » (et `attendre_fermeture`).
static SIGNAL: Condvar = Condvar::new();
/// Contexte egui de la fenêtre ouverte (pour lui demander de se redessiner).
static CONTEXTE: Mutex<Option<egui::Context>> = Mutex::new(None);
static DEMARRAGE: Once = Once::new();

impl Demandes {
    fn racine_voulue(&self) -> Option<Racine> {
        if self.fenetre {
            Some(Racine::Principale)
        } else if self.overlay {
            Some(Racine::Overlay)
        } else {
            None
        }
    }
}

/// Lance le thread « fenetre » (une seule fois ; les appels suivants ne font rien).
/// Aucune fenêtre n'est créée ici.
pub fn demarrer() {
    DEMARRAGE.call_once(|| {
        let lance = std::thread::Builder::new().name("fenetre".into()).spawn(boucle);
        if let Err(e) = lance {
            crate::log!("Interface indisponible : {e}");
            return;
        }
        // Appelée par le cerveau après CHAQUE changement d'état.
        affichage::quand_ca_change(reveil);
    });
}

/// Ouvre la fenêtre principale (ou la ramène au premier plan). Ferme l'overlay.
pub fn ouvrir() {
    modifier(|d| {
        d.fenetre = true;
        d.overlay = false;
        d.premier_plan = true;
    });
}

/// Ferme tout (fenêtre et overlay).
pub fn fermer() {
    modifier(|d| {
        d.fenetre = false;
        d.overlay = false;
    });
}

/// Affiche / masque l'overlay. L'afficher ferme la fenêtre principale.
pub fn basculer_overlay() {
    modifier(|d| {
        d.overlay = !d.overlay;
        if d.overlay {
            d.fenetre = false;
        }
    });
}

/// Bloque jusqu'à ce qu'aucune fenêtre ne soit ni demandée ni ouverte.
pub fn attendre_fermeture() {
    let mut d = DEMANDES.lock().unwrap();
    while d.racine_voulue().is_some() || d.en_cours.is_some() {
        d = SIGNAL.wait(d).unwrap();
    }
}

/// Modifie les demandes, puis réveille le thread et la fenêtre ouverte.
fn modifier(f: impl FnOnce(&mut Demandes)) {
    f(&mut DEMANDES.lock().unwrap());
    SIGNAL.notify_all();
    repeindre();
}

fn repeindre() {
    if let Some(ctx) = CONTEXTE.lock().unwrap().as_ref() {
        ctx.request_repaint();
    }
}

/// Le cerveau a changé l'état : on regarde seulement la PHASE (pas de copie de
/// tout l'état), puis on demande un redessin si une fenêtre est ouverte.
/// ATTENTION : appelée PENDANT `affichage::modifier` -> ne jamais écrire dans le
/// journal (`log!`) d'ici, ce serait un interblocage.
fn reveil() {
    let phase = affichage::phase();
    // Comparer et noter la nouvelle phase d'un seul coup (sous le verrou) : si deux
    // threads appellent `reveil` en même temps, un seul traite le changement.
    let ancienne = {
        let mut d = DEMANDES.lock().unwrap();
        std::mem::replace(&mut d.phase, phase)
    };
    if phase != ancienne {
        changement_de_phase(ancienne, phase);
    }
    repeindre();
}

fn changement_de_phase(ancienne: Option<Phase>, phase: Option<Phase>) {
    let en_partie = |p: Option<Phase>| p.is_some_and(Phase::en_partie);
    // Lecture du fichier de réglages AVANT de prendre le verrou (accès disque).
    let ouvrir_auto =
        phase == Some(Phase::ChampSelect) && crate::stockage::Reglages::charger().get("fenetre_auto") != Some("non");
    // Le raccourci n'est actif QUE pendant la partie : en dehors, Ctrl+Maj+Tab
    // reste aux autres applications (navigateur...).
    if en_partie(phase) != en_partie(ancienne) {
        crate::tray::raccourci_overlay(en_partie(phase));
    }
    modifier(|d| {
        if ouvrir_auto {
            d.fenetre = true;
            d.overlay = false;
        }
        if en_partie(ancienne) && !en_partie(phase) {
            d.overlay = false; // fin de partie : l'overlay disparaît tout seul
        }
    });
}

/// Le thread « fenetre » : dort, ouvre la fenêtre demandée, se rendort.
fn boucle() {
    loop {
        let racine = {
            let mut d = DEMANDES.lock().unwrap();
            loop {
                if let Some(r) = d.racine_voulue() {
                    d.en_cours = Some(r);
                    break r;
                }
                d = SIGNAL.wait(d).unwrap(); // 0 % CPU ici
            }
        };
        if let Err(e) = lancer(racine) {
            crate::log!("Fenêtre impossible à ouvrir : {e}");
            // Pas de nouvel essai en boucle : on oublie la demande.
            let mut d = DEMANDES.lock().unwrap();
            d.fenetre = false;
            d.overlay = false;
        }
        *CONTEXTE.lock().unwrap() = None;
        rendre_la_memoire();
        DEMANDES.lock().unwrap().en_cours = None;
        SIGNAL.notify_all();
    }
}

/// Après la fermeture : demande à Windows de sortir de la RAM les pages qui ne
/// servent plus (code du pilote graphique, polices...). Elles seront relues du
/// disque si on rouvre la fenêtre. Coût : quelques ms, une fois par fermeture.
fn rendre_la_memoire() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        // (-1, -1) = « vide mon working set » (documenté par Microsoft).
        unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
    }
    #[cfg(target_os = "linux")]
    {
        // glibc garde la mémoire libérée pour plus tard : on la lui fait rendre.
        extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        unsafe { malloc_trim(0) };
    }
}

/// Crée la fenêtre et la fait vivre jusqu'à sa fermeture (bloquant).
fn lancer(racine: Racine) -> Result<(), eframe::Error> {
    let viewport = match racine {
        Racine::Principale => egui::ViewportBuilder::default()
            .with_title("stat-traque")
            .with_inner_size([960.0, 680.0])
            .with_min_inner_size([640.0, 420.0]),
        Racine::Overlay => {
            let position = format::position_overlay(crate::stockage::Reglages::charger().get("overlay_position"));
            egui::ViewportBuilder::default()
                .with_title("stat-traque overlay")
                .with_inner_size(overlay::TAILLE)
                .with_position(position)
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_mouse_passthrough(true) // les clics passent au jeu
                .with_taskbar(false)
                .with_active(false) // ne vole pas le focus au jeu
                .with_resizable(false)
        }
    };
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        // Revenir de `run_native` à la fermeture au lieu de quitter le processus.
        run_and_return: true,
        // La boucle d'événements n'est pas sur le thread principal (pris par l'icône) :
        // winit le refuse par défaut, `with_any_thread` l'autorise. (Utilisé une seule
        // fois : eframe réutilise ensuite la même boucle dans ce thread.)
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(windows)]
            use winit::platform::windows::EventLoopBuilderExtWindows;
            #[cfg(target_os = "linux")]
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "stat-traque",
        options,
        Box::new(move |cc| {
            theme::appliquer(&cc.egui_ctx);
            *CONTEXTE.lock().unwrap() = Some(cc.egui_ctx.clone());
            Ok(Box::new(Appli::nouvelle(racine)))
        }),
    )
}

/// L'application egui (une par ouverture de fenêtre).
struct Appli {
    racine: Racine,
    /// Copie locale de l'état, refaite seulement quand `affichage::version()` change.
    etat: EtatAffichage,
    version: u64,
    ecran: Ecran,
}

impl Appli {
    fn nouvelle(racine: Racine) -> Self {
        // Lire la version AVANT l'état : si un changement arrive entre les deux,
        // on le verra au prochain passage (jamais d'état en retard).
        let version = affichage::version();
        let etat = affichage::lire();
        let ecran = Ecran::nouveau(&etat);
        Self { racine, etat, version, ecran }
    }
}

impl eframe::App for Appli {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // 1. Les demandes ont-elles changé ? (ex : overlay demandé -> on ferme la fenêtre)
        let (voulue, premier_plan) = {
            let mut d = DEMANDES.lock().unwrap();
            if self.racine == Racine::Principale && ctx.input(|i| i.viewport().close_requested()) {
                d.fenetre = false; // croix de la fenêtre
            }
            (d.racine_voulue(), std::mem::take(&mut d.premier_plan))
        };
        if voulue != Some(self.racine) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if premier_plan && self.racine == Racine::Principale {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        // 2. Copie de l'état seulement s'il a changé.
        let version = affichage::version();
        if version != self.version {
            self.version = version;
            self.etat = affichage::lire();
            self.ecran.etat_change(&self.etat);
        }
        // 3. Dessin.
        match self.racine {
            Racine::Principale => ecrans::dessiner(ui, &mut self.ecran, &self.etat),
            Racine::Overlay => overlay::dessiner(ui, &self.etat),
        }
    }

    /// Couleur de fond : opaque pour la fenêtre, transparente pour l'overlay.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        match self.racine {
            Racine::Principale => theme::FOND.to_normalized_gamma_f32(),
            Racine::Overlay => [0.0; 4],
        }
    }
}
