//! Textures des portraits de champions : au plus 16 en RAM, libérées en fin de trame.
//!
//! Principe (0 % CPU au repos, aucune boucle) :
//!  - un écran demande `texture(id, taille)` pendant qu'il dessine ;
//!  - si le portrait n'est pas encore là, on répond `None` (carré vide) et on met la
//!    demande en file ;
//!  - un thread « portraits » lit les icônes une par une (`icones::charger`, bloquant :
//!    disque, et réseau si besoin) et renvoie le résultat par un canal ;
//!  - le thread demande UN redessin quand une icône arrive ; la trame suivante la
//!    transforme en texture. Rien ne tourne entre-temps.
//!
//! La partie « logique » (cache LRU, file, échecs mémorisés) est dans `Suivi`, un type
//! sans egui ni thread, pour pouvoir la tester avec une horloge injectée.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::icones::{self, Icone, Taille};

/// Nombre maximal de textures gardées en mémoire.
const MAX_TEXTURES: usize = 16;
/// Nombre maximal de demandes en attente (en plus de celle en cours).
const MAX_FILE: usize = 16;
/// Après un échec, on ne réessaie pas ce portrait pendant ce délai.
const DELAI_ECHEC: Duration = Duration::from_secs(10 * 60);

// ---------------------------------------------------------------------------
// Structure de données pure (testable)
// ---------------------------------------------------------------------------

/// Cache LRU + file de demandes + mémoire des échecs. `K` = clé d'un portrait,
/// `V` = ce qu'on garde (ici une texture egui).
pub struct Suivi<K, V> {
    max_cache: usize,
    max_file: usize,
    /// Valeur + « date » de dernière utilisation (compteur croissant, plus parlant
    /// qu'une horloge : deux accès dans la même trame restent ordonnés).
    cache: HashMap<K, (V, u64)>,
    compteur: u64,
    file: VecDeque<K>,
    /// La requête partie vers le thread, dont on attend la réponse.
    en_cours: Option<K>,
    echecs: HashMap<K, Instant>,
}

/// Réponse à « as-tu ce portrait ? ».
#[derive(Debug, PartialEq, Eq)]
pub enum Reponse {
    /// Dans le cache (et marqué comme utilisé à l'instant).
    Pret,
    /// Pas prêt : mis en file, ou déjà en file / en cours / en échec récent.
    Absent,
}

impl<K: Hash + Eq + Clone, V> Suivi<K, V> {
    pub fn nouveau(max_cache: usize, max_file: usize) -> Self {
        Self {
            max_cache,
            max_file,
            cache: HashMap::new(),
            compteur: 0,
            file: VecDeque::new(),
            en_cours: None,
            echecs: HashMap::new(),
        }
    }

    /// Valeur en cache (et on note qu'elle vient de servir).
    pub fn lire(&mut self, k: &K) -> Option<&V> {
        self.compteur += 1;
        let c = self.compteur;
        self.cache.get_mut(k).map(|(v, vu)| {
            *vu = c;
            &*v
        })
    }

    /// Demande un portrait : s'il manque, l'ajoute à la file (sans doublon).
    pub fn demander(&mut self, k: &K, maintenant: Instant) -> Reponse {
        if self.lire(k).is_some() {
            return Reponse::Pret;
        }
        if let Some(&depuis) = self.echecs.get(k) {
            if maintenant.saturating_duration_since(depuis) < DELAI_ECHEC {
                return Reponse::Absent; // échec récent : on ne réessaie pas
            }
            self.echecs.remove(k); // expiré : nouvelle tentative permise
        }
        let deja = self.en_cours.as_ref() == Some(k) || self.file.contains(k);
        if !deja {
            if self.file.len() >= self.max_file {
                self.file.pop_front(); // la plus ancienne demande est la moins d'actualité
            }
            self.file.push_back(k.clone());
        }
        Reponse::Absent
    }

    /// Prochaine requête à envoyer au thread (une seule à la fois).
    pub fn suivant(&mut self) -> Option<K> {
        if self.en_cours.is_some() {
            return None;
        }
        let k = self.file.pop_front()?;
        self.en_cours = Some(k.clone());
        Some(k)
    }

    /// Réponse du thread : `Some(valeur)` = succès, `None` = échec mémorisé.
    pub fn recu(&mut self, k: K, valeur: Option<V>, maintenant: Instant) {
        if self.en_cours.as_ref() == Some(&k) {
            self.en_cours = None;
        }
        match valeur {
            Some(v) => {
                self.compteur += 1;
                self.cache.insert(k, (v, self.compteur));
            }
            None => {
                self.echecs.insert(k, maintenant);
            }
        }
    }

    /// Retire les moins récemment utilisés jusqu'à `max_cache` entrées.
    pub fn evincer(&mut self) {
        while self.cache.len() > self.max_cache {
            let Some(plus_vieux) = self.cache.iter().min_by_key(|(_, (_, vu))| *vu).map(|(k, _)| k.clone()) else {
                break;
            };
            self.cache.remove(&plus_vieux);
        }
    }

    /// Oublie tout (changement de patch : les anciens portraits ne valent plus).
    pub fn vider(&mut self) {
        self.cache.clear();
        self.file.clear();
        self.echecs.clear();
        self.en_cours = None;
    }

    pub fn taille_cache(&self) -> usize {
        self.cache.len()
    }
}

// ---------------------------------------------------------------------------
// Partie egui + thread
// ---------------------------------------------------------------------------

/// Clé d'un portrait : champion + taille (`Taille` n'implémente pas `Hash`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Cle {
    id_dd: String,
    fiche: bool,
}

impl Cle {
    fn taille(&self) -> Taille {
        if self.fiche { Taille::Fiche } else { Taille::Ligne }
    }
}

/// Requête envoyée au thread : (version du patch, clé).
type Requete = (String, Cle);
/// Réponse du thread : (version, clé, icône si ça a marché).
type ReponseThread = (String, Cle, Option<Icone>);

/// Cache de textures de portraits, propre à la fenêtre (détruit avec elle).
pub struct Portraits {
    ctx: egui::Context,
    version: Option<String>,
    suivi: Suivi<Cle, egui::TextureHandle>,
    /// Canaux vers/depuis le thread, créé à la première demande.
    vers_thread: Option<Sender<Requete>>,
    depuis_thread: Option<Receiver<ReponseThread>>,
}

impl Portraits {
    /// `version` : patch Data Dragon courant (None tant qu'on ne le connaît pas).
    pub fn nouveau(ctx: &egui::Context, version: Option<String>) -> Self {
        Self {
            ctx: ctx.clone(),
            version,
            suivi: Suivi::nouveau(MAX_TEXTURES, MAX_FILE),
            vers_thread: None,
            depuis_thread: None,
        }
    }

    /// Indique le patch quand il devient connu (ou change) : repart de zéro.
    pub fn definir_version(&mut self, version: Option<String>) {
        if self.version != version {
            self.version = version;
            self.suivi.vider(); // les réponses en vol d'un autre patch seront ignorées
        }
    }

    /// Texture du portrait de `id_dd`, ou `None` s'il n'est pas (encore) disponible.
    /// Dans ce cas la demande est mise en file ; un redessin sera demandé à l'arrivée.
    pub fn texture(&mut self, id_dd: &str, t: Taille) -> Option<egui::TextureId> {
        self.recevoir();
        self.version.as_ref()?; // version inconnue : rien à demander
        let cle = Cle { id_dd: id_dd.to_string(), fiche: t == Taille::Fiche };
        if self.suivi.demander(&cle, Instant::now()) == Reponse::Pret {
            return self.suivi.lire(&cle).map(egui::TextureHandle::id);
        }
        self.envoyer_suivant();
        None
    }

    /// À appeler une fois par trame : libère les textures en trop (les plus anciennes).
    pub fn fin_de_trame(&mut self) {
        self.recevoir();
        self.suivi.evincer();
    }

    /// Nombre de textures actuellement en mémoire (mesures et tests).
    pub fn nb_textures(&self) -> usize {
        self.suivi.taille_cache()
    }

    /// Transforme les icônes arrivées en textures, puis lance la demande suivante.
    fn recevoir(&mut self) {
        let mut recus = Vec::new();
        if let Some(rx) = &self.depuis_thread {
            while let Ok(r) = rx.try_recv() {
                recus.push(r);
            }
        }
        for (version, cle, icone) in recus {
            if self.version.as_deref() != Some(version.as_str()) {
                continue; // réponse d'un ancien patch
            }
            let texture = icone.and_then(|i| self.en_texture(&cle, &i));
            self.suivi.recu(cle, texture, Instant::now());
        }
        self.envoyer_suivant();
    }

    fn en_texture(&self, cle: &Cle, icone: &Icone) -> Option<egui::TextureHandle> {
        let cote = usize::from(icone.cote);
        if cote == 0 || icone.rgba.len() != cote * cote * 4 {
            return None; // icône incohérente : traitée comme un échec
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([cote, cote], &icone.rgba);
        let nom = format!("portrait:{}:{}", cle.id_dd, if cle.fiche { "fiche" } else { "ligne" });
        Some(self.ctx.load_texture(nom, image, egui::TextureOptions::LINEAR))
    }

    /// Envoie la prochaine requête au thread s'il est libre (le crée au besoin).
    fn envoyer_suivant(&mut self) {
        let Some(version) = self.version.clone() else { return };
        if self.suivi.suivant_possible() {
            self.demarrer_thread();
        }
        let Some(cle) = self.suivi.suivant() else { return };
        let envoye = self.vers_thread.as_ref().is_some_and(|tx| tx.send((version, cle.clone())).is_ok());
        if !envoye {
            // Thread mort (ne devrait pas arriver) : échec mémorisé, pas de boucle.
            self.suivi.recu(cle, None, Instant::now());
        }
    }

    /// Crée le thread « portraits » à la première demande.
    fn demarrer_thread(&mut self) {
        if self.vers_thread.is_some() {
            return;
        }
        let (tx_req, rx_req) = mpsc::channel::<Requete>();
        let (tx_rep, rx_rep) = mpsc::channel::<ReponseThread>();
        let ctx = self.ctx.clone();
        let cree = std::thread::Builder::new().name("portraits".into()).spawn(move || {
            // `for` sur le canal : le thread dort tant qu'il n'y a rien, et se termine
            // tout seul quand `Portraits` est détruit (l'émetteur est fermé).
            for (version, cle) in rx_req {
                let icone = icones::charger(&version, &cle.id_dd, cle.taille());
                let reussi = icone.is_some();
                if tx_rep.send((version, cle, icone)).is_err() {
                    break;
                }
                if reussi {
                    ctx.request_repaint(); // UN redessin : la trame crée la texture
                }
            }
        });
        if cree.is_ok() {
            self.vers_thread = Some(tx_req);
            self.depuis_thread = Some(rx_rep);
        }
    }
}

impl<K: Hash + Eq + Clone, V> Suivi<K, V> {
    /// Y a-t-il une requête prête à partir (donc besoin du thread) ?
    pub fn suivant_possible(&self) -> bool {
        self.en_cours.is_none() && !self.file.is_empty()
    }
}

/// `Portraits` partagé entre l'application et les écrans.
pub type PortraitsPartage = Arc<Mutex<Portraits>>;

/// Enregistre `Portraits` dans le contexte egui : les écrans le retrouvent avec
/// `portraits(ctx)` sans qu'on ait à changer leurs signatures.
pub fn installer(ctx: &egui::Context, p: Portraits) -> PortraitsPartage {
    let partage = Arc::new(Mutex::new(p));
    ctx.data_mut(|d| d.insert_temp(egui::Id::NULL, partage.clone()));
    partage
}

/// Depuis un écran : `if let Some(p) = textures::portraits(ui.ctx()) { p.lock().unwrap().texture(..) }`.
pub fn portraits(ctx: &egui::Context) -> Option<PortraitsPartage> {
    ctx.data(|d| d.get_temp::<PortraitsPartage>(egui::Id::NULL))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    fn suivi(cache: usize, file: usize) -> Suivi<&'static str, u32> {
        Suivi::nouveau(cache, file)
    }

    #[test]
    fn eviction_du_moins_recemment_utilise() {
        let mut s = suivi(2, 4);
        let n = t0();
        s.recu("a", Some(1), n);
        s.recu("b", Some(2), n);
        assert_eq!(s.demander(&"a", n), Reponse::Pret); // « a » devient le plus récent
        s.recu("c", Some(3), n);
        s.evincer();
        assert_eq!(s.taille_cache(), 2);
        assert_eq!(s.demander(&"a", n), Reponse::Pret);
        assert_eq!(s.demander(&"c", n), Reponse::Pret);
        assert_eq!(s.demander(&"b", n), Reponse::Absent); // « b » est parti
    }

    #[test]
    fn pas_de_doublon_en_file_ni_en_cours() {
        let mut s = suivi(2, 4);
        let n = t0();
        s.demander(&"a", n);
        s.demander(&"a", n);
        s.demander(&"b", n);
        assert_eq!(s.suivant(), Some("a"));
        assert_eq!(s.suivant(), None); // une seule requête à la fois
        s.demander(&"a", n); // « a » est en cours : pas remis en file
        s.recu("a", Some(1), n);
        assert_eq!(s.suivant(), Some("b"));
        s.recu("b", Some(2), n);
        assert_eq!(s.suivant(), None);
    }

    #[test]
    fn file_bornee_la_plus_ancienne_saute() {
        let mut s = suivi(2, 2);
        let n = t0();
        for k in ["a", "b", "c"] {
            s.demander(&k, n);
        }
        assert_eq!(s.suivant(), Some("b"));
        s.recu("b", Some(2), n);
        assert_eq!(s.suivant(), Some("c"));
    }

    #[test]
    fn echec_memorise_dix_minutes() {
        let mut s = suivi(2, 4);
        let n = t0();
        s.demander(&"a", n);
        assert_eq!(s.suivant(), Some("a"));
        s.recu("a", None, n);
        // 9 minutes plus tard : pas de nouvelle tentative.
        let tot = n + Duration::from_secs(9 * 60);
        assert_eq!(s.demander(&"a", tot), Reponse::Absent);
        assert_eq!(s.suivant(), None);
        // 11 minutes plus tard : on réessaie.
        let tard = n + Duration::from_secs(11 * 60);
        assert_eq!(s.demander(&"a", tard), Reponse::Absent);
        assert_eq!(s.suivant(), Some("a"));
    }

    #[test]
    fn vider_oublie_tout() {
        let mut s = suivi(2, 4);
        let n = t0();
        s.recu("a", Some(1), n);
        s.recu("b", None, n);
        s.demander(&"c", n);
        s.vider();
        assert_eq!(s.taille_cache(), 0);
        assert_eq!(s.suivant(), None);
        assert_eq!(s.demander(&"b", n), Reponse::Absent);
        assert_eq!(s.suivant(), Some("b")); // l'échec a été oublié
    }

    #[test]
    fn sans_version_texture_renvoie_none() {
        let ctx = egui::Context::default();
        let mut p = Portraits::nouveau(&ctx, None);
        assert!(p.texture("Ahri", Taille::Ligne).is_none());
        assert!(p.vers_thread.is_none()); // aucun thread créé pour rien
        p.fin_de_trame();
    }
}
