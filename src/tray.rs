//! Icône dans la barre des tâches (zone de notification, à côté de l'horloge).
//!
//! Écrite directement avec l'API Windows (pas de framework graphique) :
//! coût quasi nul en RAM et en taille d'exe. Une fenêtre INVISIBLE reçoit
//! les clics sur l'icône : clic gauche = ouvrir la fenêtre (ui/), clic droit =
//! petit menu (état, Ouvrir, Overlay, Quitter). Pendant la partie seulement, le
//! raccourci global Ctrl+Maj+Tab affiche / masque l'overlay. L'overlay lui-même
//! (fenêtre native GDI, ui/overlay_natif.rs) vit aussi dans ce thread.
//!
//! Hors Windows (développement sous Linux) : fonctions vides.

#[cfg(windows)]
mod windows_impl {
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::{Mutex, OnceLock};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    use crate::evenement::Evenement;

    const MSG_ICONE: u32 = WM_APP + 1; // clic sur l'icône
    const MSG_ETAT: u32 = WM_APP + 2; // l'état a changé -> mettre à jour l'infobulle
    const MSG_RACCOURCI: u32 = WM_APP + 3; // (dés)activer le raccourci de l'overlay
    const MSG_OVERLAY: u32 = WM_APP + 4; // afficher (wparam = 1) / masquer l'overlay
    const MSG_OVERLAY_MAJ: u32 = WM_APP + 5; // l'état a changé : redessiner l'overlay
    const ID_QUITTER: usize = 1;
    const ID_OUVRIR: usize = 2;
    const ID_OVERLAY: usize = 3;
    /// Identifiant de notre raccourci clavier global (choisi librement).
    const ID_RACCOURCI: i32 = 1;

    static FENETRE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());
    static ETAT: Mutex<String> = Mutex::new(String::new());
    static TX: OnceLock<Sender<Evenement>> = OnceLock::new();
    /// Message envoyé par Windows quand la barre des tâches est recréée
    /// (redémarrage de l'Explorateur) : il faut alors remettre l'icône.
    static MSG_BARRE_RECREEE: AtomicU32 = AtomicU32::new(0);
    /// Un redessin de l'overlay est déjà demandé (message posté, pas encore traité) :
    /// pendant une rafale de changements, on ne poste qu'UN message.
    static REDESSIN_EN_ATTENTE: AtomicBool = AtomicBool::new(false);

    /// Texte Rust -> texte Windows (UTF-16 terminé par un zéro).
    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn donnees_icone(fenetre: HWND) -> NOTIFYICONDATAW {
        // SAFETY : structure C "simple", tout à zéro est une valeur valide.
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = fenetre;
        nid.uID = 1;
        nid
    }

    fn mettre_infobulle(nid: &mut NOTIFYICONDATAW) {
        let texte = format!("stat-traque — {}", ETAT.lock().unwrap());
        // 127 unités UTF-16 max (+ le zéro final) ; on coupe entre deux caractères,
        // jamais au milieu d'un caractère codé sur 2 unités.
        let mut i = 0;
        for c in texte.chars() {
            let mut tampon = [0u16; 2];
            let unites = c.encode_utf16(&mut tampon);
            if i + unites.len() > 127 {
                break;
            }
            nid.szTip[i..i + unites.len()].copy_from_slice(unites);
            i += unites.len();
        }
        nid.szTip[i] = 0;
        nid.uFlags |= NIF_TIP;
    }

    unsafe fn ajouter_icone(fenetre: HWND) {
        let mut nid = donnees_icone(fenetre);
        nid.uFlags = NIF_ICON | NIF_MESSAGE;
        nid.uCallbackMessage = MSG_ICONE;
        nid.hIcon = LoadIconW(null_mut(), IDI_APPLICATION); // icône provisoire
        mettre_infobulle(&mut nid);
        Shell_NotifyIconW(NIM_ADD, &nid);
    }

    unsafe extern "system" fn procedure(fenetre: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let barre_recreee = MSG_BARRE_RECREEE.load(Ordering::Relaxed);
        if barre_recreee != 0 && msg == barre_recreee {
            ajouter_icone(fenetre);
            return 0;
        }
        match msg {
            MSG_ICONE if lparam as u32 == WM_LBUTTONUP => {
                crate::ui::ouvrir();
                0
            }
            MSG_ICONE if lparam as u32 == WM_RBUTTONUP => {
                afficher_menu(fenetre);
                0
            }
            MSG_RACCOURCI => {
                activer_raccourci(fenetre, wparam != 0);
                0
            }
            MSG_OVERLAY => {
                crate::ui::overlay_natif::afficher(wparam != 0);
                0
            }
            MSG_OVERLAY_MAJ => {
                // Remis à faux AVANT de lire l'état : un changement arrivé pendant le
                // redessin postera un nouveau message (rien n'est perdu).
                REDESSIN_EN_ATTENTE.store(false, Ordering::Release);
                crate::ui::overlay_natif::redessiner();
                0
            }
            WM_HOTKEY if wparam as i32 == ID_RACCOURCI => {
                crate::ui::basculer_overlay();
                0
            }
            MSG_ETAT => {
                let mut nid = donnees_icone(fenetre);
                mettre_infobulle(&mut nid);
                Shell_NotifyIconW(NIM_MODIFY, &nid);
                0
            }
            _ => DefWindowProcW(fenetre, msg, wparam, lparam),
        }
    }

    unsafe fn afficher_menu(fenetre: HWND) {
        let menu = CreatePopupMenu();
        let etat = utf16(&ETAT.lock().unwrap());
        let quitter = utf16("Quitter");
        let ouvrir = utf16("Ouvrir stat-traque");
        let overlay = utf16("Afficher / masquer l'overlay\tCtrl+Maj+Tab");
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, etat.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());
        AppendMenuW(menu, MF_STRING, ID_OUVRIR, ouvrir.as_ptr());
        SetMenuDefaultItem(menu, ID_OUVRIR as u32, 0); // en gras : c'est le clic gauche
        // L'overlay n'a de sens que pendant la partie : grisé sinon.
        let en_partie = crate::affichage::phase().is_some_and(crate::gameflow::Phase::en_partie);
        AppendMenuW(menu, if en_partie { MF_STRING } else { MF_STRING | MF_GRAYED }, ID_OVERLAY, overlay.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());
        AppendMenuW(menu, MF_STRING, ID_QUITTER, quitter.as_ptr());
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        SetForegroundWindow(fenetre); // sinon le menu ne se ferme pas en cliquant ailleurs
        let choix = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD, p.x, p.y, 0, fenetre, null_mut());
        DestroyMenu(menu);
        // Recommandé par Microsoft après TrackPopupMenu (sinon le 2e clic peut être ignoré).
        PostMessageW(fenetre, WM_NULL, 0, 0);
        match choix as usize {
            ID_OUVRIR => crate::ui::ouvrir(),
            ID_OVERLAY => crate::ui::basculer_overlay(),
            ID_QUITTER => {
                if let Some(tx) = TX.get() {
                    let _ = tx.send(Evenement::Quitter);
                }
                crate::ui::overlay_natif::afficher(false);
                Shell_NotifyIconW(NIM_DELETE, &donnees_icone(fenetre));
                PostQuitMessage(0); // main.rs ferme ensuite la fenêtre (processus enfant)
            }
            _ => {}
        }
    }

    /// Crée l'icône puis traite les messages Windows jusqu'à "Quitter".
    pub fn lancer(tx: Sender<Evenement>) {
        let _ = TX.set(tx);
        unsafe {
            // Écrans à 125 % / 150 % : on dessine nous-mêmes à la bonne taille (texte net)
            // au lieu de laisser Windows agrandir une image floue. (L'overlay adapte ses
            // tailles à l'écran ; la fenêtre egui est un autre processus, déjà adaptée.)
            use windows_sys::Win32::UI::HiDpi::{
                SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            };
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(null_mut());
            let classe = utf16("stat-traque");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: classe.as_ptr(),
                // SAFETY : les autres champs à zéro = valeurs par défaut.
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            let nom_msg = utf16("TaskbarCreated");
            MSG_BARRE_RECREEE.store(RegisterWindowMessageW(nom_msg.as_ptr()), Ordering::Relaxed);
            let fenetre = CreateWindowExW(
                0, classe.as_ptr(), classe.as_ptr(), 0, 0, 0, 0, 0,
                null_mut(), null_mut(), instance, null_mut(),
            );
            FENETRE.store(fenetre, Ordering::Release);
            // Lancé pendant une partie ? Le raccourci doit marcher tout de suite
            // (le changement de phase a pu arriver avant que cette fenêtre existe).
            if crate::affichage::phase().is_some_and(crate::gameflow::Phase::en_partie) {
                activer_raccourci(fenetre, true);
            }

            ajouter_icone(fenetre);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// Enregistre / retire le raccourci global Ctrl+Maj+Tab. Doit se faire dans le
    /// thread de l'icône (Windows envoie WM_HOTKEY à la fenêtre qui l'a enregistré).
    unsafe fn activer_raccourci(fenetre: HWND, actif: bool) {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_TAB,
        };
        if actif {
            // Échec possible si une autre application a déjà pris ce raccourci :
            // le menu de l'icône permet toujours d'afficher l'overlay.
            let ok = RegisterHotKey(fenetre, ID_RACCOURCI, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, VK_TAB as u32);
            if ok == 0 {
                crate::log!("Raccourci Ctrl+Maj+Tab déjà pris par une autre application : overlay par le menu de l'icône.");
            }
        } else {
            UnregisterHotKey(fenetre, ID_RACCOURCI);
        }
    }

    /// Active le raccourci de l'overlay (pendant la partie) ou le libère.
    /// Peut être appelée depuis n'importe quel thread (ne fait que poster un message).
    pub fn raccourci_overlay(actif: bool) {
        let fenetre = FENETRE.load(Ordering::Acquire);
        if !fenetre.is_null() {
            unsafe { PostMessageW(fenetre, MSG_RACCOURCI, actif as usize, 0) };
        }
    }

    /// Affiche / masque l'overlay natif. N'importe quel thread : on poste un message,
    /// l'overlay est créé / détruit dans le thread de l'icône.
    pub fn afficher_overlay(visible: bool) {
        poster(MSG_OVERLAY, visible as usize);
    }

    /// Redessine l'overlay (s'il est affiché). N'importe quel thread.
    pub fn redessiner_overlay() {
        if !REDESSIN_EN_ATTENTE.swap(true, Ordering::AcqRel) {
            poster(MSG_OVERLAY_MAJ, 0);
        }
    }

    fn poster(msg: u32, wparam: usize) {
        let fenetre = FENETRE.load(Ordering::Acquire);
        if !fenetre.is_null() {
            unsafe { PostMessageW(fenetre, msg, wparam, 0) };
        }
    }

    /// Peut être appelée depuis n'importe quel thread.
    pub fn definir_etat(texte: &str) {
        *ETAT.lock().unwrap() = texte.to_string();
        let fenetre = FENETRE.load(Ordering::Acquire);
        if !fenetre.is_null() {
            // PostMessage : la mise à jour se fait dans le thread de la fenêtre.
            unsafe { PostMessageW(fenetre, MSG_ETAT, 0, 0) };
        }
    }

    /// Une seule instance à la fois. Renvoie `false` si stat-traque tourne déjà.
    /// (Le verrou nommé disparaît tout seul quand le processus se termine.)
    pub fn instance_unique() -> bool {
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let nom = utf16("Local\\stat-traque-instance");
        unsafe {
            CreateMutexW(null_mut(), 0, nom.as_ptr());
            GetLastError() != ERROR_ALREADY_EXISTS
        }
    }

    /// Évite les fenêtres « Aucun disque dans le lecteur » si un chemin pointe vers
    /// un lecteur vide ou débranché : l'erreur est simplement renvoyée au programme.
    pub fn sans_boites_erreur_disque() {
        use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS};
        unsafe { SetErrorMode(SEM_FAILCRITICALERRORS) };
    }

    /// Mode `--console` : ouvre une console À PART. Ctrl+C ou fermer cette fenêtre
    /// arrête l'app (avec la console du terminal, Windows n'attend pas l'app et
    /// Ctrl+C ne l'arrêtait pas).
    pub fn nouvelle_console() {
        use windows_sys::Win32::System::Console::{AllocConsole, FreeConsole, SetConsoleOutputCP};
        // 65001 = UTF-8. Sans ça, une console Windows lit nos textes UTF-8 avec l'ancienne
        // page de code (1252/850) : « indigo » accentué devient « indÃ¬go ».
        // Sans risque ici : c'est NOTRE console (créée par AllocConsole), pas celle du
        // terminal de l'utilisateur. Coût : un appel système au démarrage, 0 RAM.
        const UTF8: u32 = 65001;
        unsafe {
            FreeConsole();
            AllocConsole();
            SetConsoleOutputCP(UTF8);
        }
    }

    /// Commandes courtes (`cle`, `region`, `aide`) : écrit dans le terminal d'où on lance.
    pub fn ouvrir_console() {
        use windows_sys::Win32::System::Console::{AllocConsole, AttachConsole, ATTACH_PARENT_PROCESS};
        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                AllocConsole();
            }
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{
    afficher_overlay, definir_etat, instance_unique, lancer, nouvelle_console, ouvrir_console, raccourci_overlay,
    redessiner_overlay, sans_boites_erreur_disque,
};

#[cfg(not(windows))]
pub fn definir_etat(_texte: &str) {}

#[cfg(not(windows))]
pub fn ouvrir_console() {}

#[cfg(not(windows))]
pub fn raccourci_overlay(_actif: bool) {}

#[cfg(not(windows))]
pub fn nouvelle_console() {}

#[cfg(not(windows))]
pub fn instance_unique() -> bool {
    true
}

#[cfg(not(windows))]
pub fn sans_boites_erreur_disque() {}

#[cfg(not(windows))]
pub fn lancer(_tx: std::sync::mpsc::Sender<crate::evenement::Evenement>) {}
