//! Icône dans la barre des tâches (zone de notification, à côté de l'horloge).
//!
//! Écrite directement avec l'API Windows (pas de framework graphique) :
//! coût quasi nul en RAM et en taille d'exe. Une fenêtre INVISIBLE reçoit
//! les clics sur l'icône ; un clic affiche un petit menu (état + Quitter).
//!
//! Hors Windows (développement sous Linux) : fonctions vides.

#[cfg(windows)]
mod windows_impl {
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicPtr, Ordering};
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
    const ID_QUITTER: usize = 1;

    static FENETRE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());
    static ETAT: Mutex<String> = Mutex::new(String::new());
    static TX: OnceLock<Sender<Evenement>> = OnceLock::new();

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
        for (dst, src) in nid.szTip.iter_mut().zip(utf16(&texte).iter().take(127)) {
            *dst = *src;
        }
        nid.uFlags |= NIF_TIP;
    }

    unsafe extern "system" fn procedure(fenetre: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            MSG_ICONE if matches!(lparam as u32, WM_LBUTTONUP | WM_RBUTTONUP) => {
                afficher_menu(fenetre);
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
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, etat.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());
        AppendMenuW(menu, MF_STRING, ID_QUITTER, quitter.as_ptr());
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        SetForegroundWindow(fenetre); // sinon le menu ne se ferme pas en cliquant ailleurs
        let choix = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD, p.x, p.y, 0, fenetre, null_mut());
        DestroyMenu(menu);
        if choix as usize == ID_QUITTER {
            if let Some(tx) = TX.get() {
                let _ = tx.send(Evenement::Quitter);
            }
            Shell_NotifyIconW(NIM_DELETE, &donnees_icone(fenetre));
            PostQuitMessage(0);
        }
    }

    /// Crée l'icône puis traite les messages Windows jusqu'à "Quitter".
    pub fn lancer(tx: Sender<Evenement>) {
        let _ = TX.set(tx);
        unsafe {
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
            let fenetre = CreateWindowExW(
                0, classe.as_ptr(), classe.as_ptr(), 0, 0, 0, 0, 0,
                null_mut(), null_mut(), instance, null_mut(),
            );
            FENETRE.store(fenetre, Ordering::Release);

            let mut nid = donnees_icone(fenetre);
            nid.uFlags = NIF_ICON | NIF_MESSAGE;
            nid.uCallbackMessage = MSG_ICONE;
            nid.hIcon = LoadIconW(null_mut(), IDI_APPLICATION); // icône provisoire
            mettre_infobulle(&mut nid);
            Shell_NotifyIconW(NIM_ADD, &nid);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
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

    /// En mode `--console` (ou commande `cle`), ouvre une console pour voir les messages.
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
pub use windows_impl::{definir_etat, lancer, ouvrir_console};

#[cfg(not(windows))]
pub fn definir_etat(_texte: &str) {}

#[cfg(not(windows))]
pub fn ouvrir_console() {}

#[cfg(not(windows))]
pub fn lancer(_tx: std::sync::mpsc::Sender<crate::evenement::Evenement>) {}
