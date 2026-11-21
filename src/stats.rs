//! Statistiques par champion et par rôle, sur le patch actuel : runes, sorts,
//! matchups de lane, objets, et tier list.
//!
//! D'où viennent les chiffres ? Blitz a des serveurs qui analysent des millions de
//! parties. Nous, on fait pareil SANS serveur :
//!  - `stat-traque collecter` lit des parties classées Master+ via l'API Riot
//!    (voir collecte.rs) et les compte ici ;
//!  - une tâche GitHub automatique le fait chaque jour et publie le fichier ;
//!  - l'app télécharge ce fichier (quelques centaines de Ko) une fois par jour.
//!
//! Optimisation : l'app ne charge JAMAIS tout le fichier en RAM. Quand tu verrouilles
//! un champion, elle lit le fichier ligne par ligne et ne garde que les lignes des
//! champions utiles (le tien + les ennemis visibles) : quelques dizaines de Ko.
//! La tier list parcourt tout le fichier, mais ne garde que les lignes `C` et `B`
//! (une par champion et par rôle : ~900 lignes, ~40 Ko).
//!
//! Format (TSV). Les lignes inconnues sont ignorées : un ancien stat-traque sait
//! lire un fichier récent (il saute les nouvelles lignes) et inversement.
//!   #patch<TAB>16.23
//!   #parties<TAB>nombre de parties comptées                       (nouveau)
//!   P<TAB>champion<TAB>rôle<TAB>style1<TAB>style2<TAB>9 perks (virgules)<TAB>parties<TAB>victoires
//!   S<TAB>champion<TAB>rôle<TAB>sort1<TAB>sort2<TAB>parties<TAB>victoires
//!   C<TAB>champion<TAB>rôle<TAB>parties<TAB>victoires              (nouveau : total du champion au rôle)
//!   M<TAB>champion<TAB>rôle<TAB>adversaire<TAB>parties<TAB>victoires   (nouveau : matchup de lane)
//!   O<TAB>champion<TAB>rôle<TAB>objet<TAB>L|B<TAB>parties<TAB>victoires (nouveau : légendaire ou bottes)
//!   B<TAB>champion<TAB>parties où il a été banni                   (nouveau)

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ddragon::GenreObjet;

/// Rôles, au format du client LoL (`assignedPosition`).
pub const ROLES: [&str; 5] = ["top", "jungle", "middle", "bottom", "utility"];

/// match-v5 écrit "TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY".
fn role_depuis_match(position: &str) -> Option<&'static str> {
    ROLES.iter().copied().find(|r| r.eq_ignore_ascii_case(position))
}

/// Rôle tapé par l'utilisateur (`stat-traque tierlist mid`) : on accepte les
/// noms courants ("mid", "adc", "support"...).
pub fn role_depuis_texte(texte: &str) -> Option<&'static str> {
    let t = texte.to_ascii_lowercase();
    let role = match t.as_str() {
        "top" | "haut" => "top",
        "jungle" | "jgl" | "jungler" => "jungle",
        "mid" | "middle" | "milieu" => "middle",
        "bot" | "bottom" | "adc" | "carry" => "bottom",
        "sup" | "supp" | "support" | "utility" => "utility",
        _ => return None,
    };
    Some(role)
}

/// Une page de runes complète, dans l'ordre attendu par le client :
/// 4 runes principales, 2 secondaires, 3 fragments (attaque, flexible, défense).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageRunes {
    pub principal: u16,
    pub secondaire: u16,
    pub perks: [u16; 9],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Compteur {
    pub parties: u32,
    pub victoires: u32,
}

impl Compteur {
    fn ajouter(&mut self, victoire: bool) {
        self.parties += 1;
        self.victoires += victoire as u32;
    }
    fn fusionner(&mut self, autre: Compteur) {
        self.parties += autre.parties;
        self.victoires += autre.victoires;
    }
    pub fn winrate(&self) -> f32 {
        if self.parties == 0 { 0.0 } else { self.victoires as f32 * 100.0 / self.parties as f32 }
    }
    /// Winrate « prudent » : on ajoute `poids` parties fictives au winrate moyen
    /// `moyenne` du champion. Une page jouée 10 fois reste proche de la moyenne ;
    /// une page jouée 2 000 fois garde son vrai winrate.
    fn prudent(&self, moyenne: f64, poids: f64) -> f64 {
        (self.victoires as f64 + poids * moyenne) / (self.parties as f64 + poids)
    }
    /// Le même duel vu par l'AUTRE champion : ses victoires sont nos défaites
    /// (il n'y a pas de match nul dans LoL).
    fn inverse(&self) -> Compteur {
        Compteur { parties: self.parties, victoires: self.parties - self.victoires.min(self.parties) }
    }
}

/// Clé : (champion, rôle). Le rôle est un `&'static str` qui pointe dans `ROLES` :
/// pas de texte copié par ligne.
type Cle = (u16, &'static str);

#[derive(Debug, Default)]
pub struct Stats {
    pub patch: String,
    pages: HashMap<Cle, HashMap<PageRunes, Compteur>>,
    /// Paires de sorts rangées (petit id d'abord) : Flash sur D ou F = même choix.
    sorts: HashMap<Cle, HashMap<(u16, u16), Compteur>>,
    /// Nombre de parties comptées (pour le pick rate et le ban rate).
    parties: u32,
    /// Total de chaque champion à chaque rôle (lignes `C`).
    totaux: HashMap<Cle, Compteur>,
    /// Matchups de lane : (champion, rôle) -> adversaire -> résultat du champion.
    matchups: HashMap<Cle, HashMap<u16, Compteur>>,
    /// Objets légendaires présents en fin de partie.
    legendaires: HashMap<Cle, HashMap<u32, Compteur>>,
    /// Bottes présentes en fin de partie.
    bottes: HashMap<Cle, HashMap<u32, Compteur>>,
    /// Nombre de parties où chaque champion a été banni (une fois par partie).
    bans: HashMap<u16, u32>,
}

/// Lit la page de runes d'un participant match-v5.
fn page_depuis_match(perks: &Value) -> Option<PageRunes> {
    let styles = perks["styles"].as_array()?;
    let principal = styles.iter().find(|s| s["description"] == "primaryStyle")?;
    let secondaire = styles.iter().find(|s| s["description"] == "subStyle")?;
    let mut ids = Vec::with_capacity(9);
    for style in [principal, secondaire] {
        ids.extend(style["selections"].as_array()?.iter().filter_map(|s| s["perk"].as_u64()).map(|p| p as u16));
    }
    let fragments = &perks["statPerks"];
    for k in ["offense", "flex", "defense"] {
        ids.push(fragments[k].as_u64()? as u16);
    }
    Some(PageRunes {
        principal: principal["style"].as_u64()? as u16,
        secondaire: secondaire["style"].as_u64()? as u16,
        perks: ids.try_into().ok()?,
    })
}

/// Les 6 cases d'objets d'un participant (item6 = la balise, on l'ignore).
const CASES_OBJETS: [&str; 6] = ["item0", "item1", "item2", "item3", "item4", "item5"];

/// Une ligne de matchup ou de contre-pick : `champion` est « l'autre » champion
/// (l'adversaire pour `matchups`, le champion conseillé pour `contre_picks`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LigneMatchup {
    pub champion: u16,
    /// Résultat vu de MON côté (mes victoires).
    pub stats: Compteur,
    /// Winrate lissé (0 à 100) qui sert au tri.
    pub score: f32,
}

/// Objets conseillés pour un champion (fréquences en fin de partie).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConseilObjets {
    /// Les 3 légendaires les plus construits, du plus fréquent au moins fréquent.
    pub core: Vec<(u32, Compteur)>,
    /// Légendaires suivants (jusqu'à 5).
    pub situationnels: Vec<(u32, Compteur)>,
    /// Les bottes les plus fréquentes (jusqu'à 2).
    pub bottes: Vec<(u32, Compteur)>,
    /// Parties du champion (au rôle) : sert à calculer « construit dans X % des parties ».
    pub parties: u32,
    /// Rôle des stats (None = tous rôles confondus, faute de données).
    pub role: Option<String>,
}

impl ConseilObjets {
    /// Part des parties où l'objet est présent en fin de partie, en %.
    pub fn frequence(&self, c: &Compteur) -> f32 {
        c.parties as f32 * 100.0 / self.parties.max(1) as f32
    }

    /// Blocs (titre, objets) pour l'item set du client, dans l'ordre d'achat conseillé.
    pub fn blocs(&self) -> Vec<(String, Vec<u32>)> {
        let ids = |v: &[(u32, Compteur)]| v.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        let portee = self.role.as_deref().map_or("tous rôles".to_string(), String::from);
        let mut blocs = Vec::new();
        for (titre, liste) in [
            (format!("Core ({} parties, {portee})", self.parties), &self.core),
            ("Bottes".to_string(), &self.bottes),
            ("Situationnels".to_string(), &self.situationnels),
        ] {
            if !liste.is_empty() {
                blocs.push((titre, ids(liste)));
            }
        }
        blocs
    }
}

/// Une ligne de la tier list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LigneTier {
    pub champion: u16,
    pub role: &'static str,
    pub stats: Compteur,
    /// % des parties où ce champion est joué à ce rôle.
    pub pick_rate: f32,
    /// % des parties où ce champion est banni (tous rôles).
    pub ban_rate: f32,
    /// Winrate lissé (0 à 100) : sert au tri.
    pub score: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TierList {
    pub patch: String,
    pub parties: u32,
    /// Triées par rôle (ordre de `ROLES`) puis par score décroissant.
    pub lignes: Vec<LigneTier>,
}

impl TierList {
    pub fn du_role<'a>(&'a self, role: &'a str) -> impl Iterator<Item = &'a LigneTier> + 'a {
        self.lignes.iter().filter(move |l| l.role == role)
    }
}

// Réglages des calculs (choisis pour un échantillon de quelques milliers de parties).
/// Parties fictives ajoutées pour lisser un matchup (moins que pour une page de
/// runes : un matchup précis a beaucoup moins de parties).
const POIDS_MATCHUP: f64 = 30.0;
/// Parties fictives à 50 % ajoutées pour lisser la tier list.
const POIDS_TIER: f64 = 100.0;
/// En dessous, un rôle est trop peu joué pour qu'on se fie à ses seules stats.
const MIN_PARTIES_ROLE: u32 = 30;

/// Taille approximative d'une table en RAM (cases réservées × taille d'une case).
fn taille_table<K, V>(m: &HashMap<K, V>) -> usize {
    m.capacity() * (std::mem::size_of::<K>() + std::mem::size_of::<V>() + 1)
}

/// Ajoute `cle -> sous-cle -> compteur` (évite de répéter les `entry`).
fn compter<K: std::hash::Hash + Eq>(
    table: &mut HashMap<Cle, HashMap<K, Compteur>>,
    cle: Cle,
    sous_cle: K,
    c: Compteur,
) {
    table.entry(cle).or_default().entry(sous_cle).or_default().fusionner(c);
}

/// Fusionne les sous-tables d'un champion : un rôle précis, ou tous ses rôles.
fn regrouper<K: std::hash::Hash + Eq + Copy>(
    table: &HashMap<Cle, HashMap<K, Compteur>>,
    champion: u16,
    role: Option<&str>,
) -> HashMap<K, Compteur> {
    let mut tout: HashMap<K, Compteur> = HashMap::new();
    for ((c, r), m) in table {
        if *c == champion && role.is_none_or(|ro| ro == *r) {
            for (k, n) in m {
                tout.entry(*k).or_default().fusionner(*n);
            }
        }
    }
    tout
}

impl Stats {
    pub fn nouveau(patch: &str) -> Self {
        Self { patch: patch.to_string(), ..Default::default() }
    }

    /// Compte les 10 participants d'une partie match-v5. Renvoie false si la
    /// partie n'est pas du bon patch (ou illisible).
    /// `genre` dit si un objet est légendaire / des bottes (Data Dragon) : les autres
    /// objets (composants, potions...) ne sont pas comptés.
    pub fn ajouter_partie(&mut self, partie: &Value, genre: &dyn Fn(u32) -> Option<GenreObjet>) -> bool {
        let info = &partie["info"];
        let version = info["gameVersion"].as_str().unwrap_or("");
        if !version.starts_with(&format!("{}.", self.patch)) {
            return false;
        }
        let Some(participants) = info["participants"].as_array() else { return false };
        self.parties += 1;
        // Qui joue où : (équipe, rôle, champion). Sert à trouver l'adversaire de lane.
        let places: Vec<(u64, &'static str, u16)> = participants
            .iter()
            .filter_map(|p| {
                let role = p["teamPosition"].as_str().and_then(role_depuis_match)?;
                Some((p["teamId"].as_u64()?, role, p["championId"].as_u64()? as u16))
            })
            .collect();
        for p in participants {
            let (Some(champion), Some(role)) =
                (p["championId"].as_u64(), p["teamPosition"].as_str().and_then(role_depuis_match))
            else {
                continue;
            };
            let cle = (champion as u16, role);
            let victoire = p["win"] == true;
            let un = Compteur { parties: 1, victoires: victoire as u32 };
            self.totaux.entry(cle).or_default().ajouter(victoire);
            if let Some(page) = page_depuis_match(&p["perks"]) {
                compter(&mut self.pages, cle, page, un);
            }
            if let (Some(a), Some(b)) = (p["summoner1Id"].as_u64(), p["summoner2Id"].as_u64()) {
                let paire = (a.min(b) as u16, a.max(b) as u16);
                compter(&mut self.sorts, cle, paire, un);
            }
            // Adversaire de lane = l'UNIQUE joueur de l'autre équipe au même rôle.
            if let Some(equipe) = p["teamId"].as_u64() {
                let mut en_face = places.iter().filter(|(e, r, _)| *e != equipe && *r == role);
                if let (Some(&(_, _, adversaire)), None) = (en_face.next(), en_face.next()) {
                    compter(&mut self.matchups, cle, adversaire, un);
                }
            }
            // Objets de fin de partie, chacun compté une fois (deux fois le même
            // objet = une seule « construction »).
            let mut vus: Vec<u32> = Vec::with_capacity(6);
            for case in CASES_OBJETS {
                if let Some(id) = p[case].as_u64().map(|i| i as u32).filter(|&i| i != 0 && !vus.contains(&i)) {
                    vus.push(id);
                    match genre(id) {
                        Some(GenreObjet::Legendaire) => compter(&mut self.legendaires, cle, id, un),
                        Some(GenreObjet::Bottes) => compter(&mut self.bottes, cle, id, un),
                        None => {}
                    }
                }
            }
        }
        // Bans : un champion banni par les deux équipes ne compte qu'une fois.
        let mut bannis: Vec<u16> = info["teams"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|t| t["bans"].as_array().into_iter().flatten())
            .filter_map(|b| b["championId"].as_i64())
            .filter(|&id| id > 0) // -1 = pas de ban
            .map(|id| id as u16)
            .collect();
        bannis.sort_unstable();
        bannis.dedup();
        for c in bannis {
            *self.bans.entry(c).or_default() += 1;
        }
        true
    }

    /// Texte du fichier. `min_parties` : on n'écrit pas les combinaisons trop rares
    /// (les totaux `C` et les bans `B` sont toujours écrits).
    pub fn vers_texte(&self, min_parties: u32) -> String {
        let mut lignes = vec![format!("#patch\t{}", self.patch), format!("#parties\t{}", self.parties)];
        let mut cles: Vec<&Cle> = self
            .pages
            .keys()
            .chain(self.sorts.keys())
            .chain(self.totaux.keys())
            .chain(self.matchups.keys())
            .chain(self.legendaires.keys())
            .chain(self.bottes.keys())
            .collect();
        cles.sort();
        cles.dedup();
        let assez = |c: &&Compteur| c.parties >= min_parties;
        // Sous-table triée par id (fichier stable d'une collecte à l'autre).
        fn triee<K: Copy + Ord>(m: Option<&HashMap<K, Compteur>>) -> Vec<(K, Compteur)> {
            let mut v: Vec<(K, Compteur)> = m.into_iter().flatten().map(|(k, c)| (*k, *c)).collect();
            v.sort_by_key(|(k, _)| *k);
            v
        }
        for cle in cles {
            let (ch, ro) = *cle;
            if let Some(c) = self.totaux.get(cle) {
                lignes.push(format!("C\t{ch}\t{ro}\t{}\t{}", c.parties, c.victoires));
            }
            for (page, c) in self.pages.get(cle).into_iter().flatten().filter(|(_, c)| assez(c)) {
                let perks: Vec<String> = page.perks.iter().map(u16::to_string).collect();
                lignes.push(format!(
                    "P\t{ch}\t{ro}\t{}\t{}\t{}\t{}\t{}",
                    page.principal, page.secondaire, perks.join(","), c.parties, c.victoires
                ));
            }
            for ((a, b), c) in self.sorts.get(cle).into_iter().flatten().filter(|(_, c)| assez(c)) {
                lignes.push(format!("S\t{ch}\t{ro}\t{a}\t{b}\t{}\t{}", c.parties, c.victoires));
            }
            for (adv, c) in triee(self.matchups.get(cle)).iter().filter(|(_, c)| assez(&c)) {
                lignes.push(format!("M\t{ch}\t{ro}\t{adv}\t{}\t{}", c.parties, c.victoires));
            }
            for (table, lettre) in [(&self.legendaires, 'L'), (&self.bottes, 'B')] {
                for (id, c) in triee(table.get(cle)).iter().filter(|(_, c)| assez(&c)) {
                    lignes.push(format!("O\t{ch}\t{ro}\t{id}\t{lettre}\t{}\t{}", c.parties, c.victoires));
                }
            }
        }
        let mut bans: Vec<(&u16, &u32)> = self.bans.iter().collect();
        bans.sort();
        lignes.extend(bans.into_iter().map(|(c, n)| format!("B\t{c}\t{n}")));
        lignes.join("\n")
    }

    /// Lit une ligne du fichier. `garder(type, champion)` choisit ce qu'on garde en RAM.
    fn lire_ligne(&mut self, ligne: &str, garder: &dyn Fn(&str, u16) -> bool) {
        let c: Vec<&str> = ligne.split('\t').collect();
        match c[0] {
            "#patch" => {
                self.patch = c.get(1).unwrap_or(&"").to_string();
                return;
            }
            "#parties" => {
                self.parties = c.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
                return;
            }
            _ => {}
        }
        let Some(champion) = c.get(1).and_then(|x| x.parse::<u16>().ok()) else { return };
        if !garder(c[0], champion) {
            return;
        }
        let n = |i: usize| c.get(i).and_then(|x| x.parse::<u32>().ok());
        if let ("B", 3, Some(bans)) = (c[0], c.len(), n(2)) {
            *self.bans.entry(champion).or_default() += bans;
            return;
        }
        let Some(role) = c.get(2).and_then(|r| ROLES.iter().copied().find(|x| x == r)) else { return };
        let cle = (champion, role);
        let compteur = |i: usize| Some(Compteur { parties: n(i)?, victoires: n(i + 1)? });
        match (c[0], c.len()) {
            ("P", 8) => {
                let perks: Vec<u16> = c[5].split(',').filter_map(|p| p.parse().ok()).collect();
                let (Some(p1), Some(p2), Ok(perks), Some(nb)) = (n(3), n(4), <[u16; 9]>::try_from(perks), compteur(6))
                else {
                    return;
                };
                compter(&mut self.pages, cle, PageRunes { principal: p1 as u16, secondaire: p2 as u16, perks }, nb);
            }
            ("S", 7) => {
                if let (Some(a), Some(b), Some(nb)) = (n(3), n(4), compteur(5)) {
                    compter(&mut self.sorts, cle, (a as u16, b as u16), nb);
                }
            }
            ("C", 5) => {
                if let Some(nb) = compteur(3) {
                    self.totaux.entry(cle).or_default().fusionner(nb);
                }
            }
            ("M", 6) => {
                if let (Some(adv), Some(nb)) = (n(3), compteur(4)) {
                    compter(&mut self.matchups, cle, adv as u16, nb);
                }
            }
            ("O", 7) => {
                let table = match c[4] {
                    "L" => &mut self.legendaires,
                    "B" => &mut self.bottes,
                    _ => return,
                };
                if let (Some(id), Some(nb)) = (n(3), compteur(5)) {
                    compter(table, cle, id, nb);
                }
            }
            _ => {}
        }
    }

    /// Compatibilité avec les fichiers d'avant les lignes `C` / `#parties` :
    /// on reconstruit ces totaux à partir des pages de runes.
    fn completer_ancien_format(&mut self) {
        if self.totaux.is_empty() {
            for (cle, m) in &self.pages {
                for c in m.values() {
                    self.totaux.entry(*cle).or_default().fusionner(*c);
                }
            }
        }
        if self.parties == 0 {
            // Chaque partie compte 10 participants.
            self.parties = self.totaux.values().map(|c| c.parties).sum::<u32>() / 10;
        }
    }

    /// Lit un texte complet. `seulement` : ne garder qu'un champion (lecture économe en RAM).
    pub fn depuis_texte(texte: &str, seulement: Option<u16>) -> Self {
        let mut s = Stats::default();
        for ligne in texte.lines() {
            s.lire_ligne(ligne, &|_, c| seulement.is_none_or(|id| id == c));
        }
        s.completer_ancien_format();
        s
    }

    /// Lit le fichier LIGNE PAR LIGNE (jamais en entier en RAM) et ne garde que les
    /// lignes des `champions` demandés. C'est la lecture du champ select.
    pub fn charger_filtre(chemin: &Path, champions: &[u16]) -> Option<Self> {
        let mut s = Stats::default();
        lire_lignes(chemin, |l| s.lire_ligne(l, &|_, c| champions.contains(&c)))?;
        s.completer_ancien_format();
        Some(s)
    }

    pub fn nombre_parties_vues(&self) -> u32 {
        self.parties
    }

    /// RAM occupée par les tables (approximation : cases réservées des tables).
    pub fn memoire_estimee(&self) -> usize {
        fn double<K, K2, V>(m: &HashMap<K, HashMap<K2, V>>) -> usize {
            taille_table(m) + m.values().map(taille_table).sum::<usize>()
        }
        double(&self.pages)
            + double(&self.sorts)
            + double(&self.matchups)
            + double(&self.legendaires)
            + double(&self.bottes)
            + taille_table(&self.totaux)
            + taille_table(&self.bans)
    }

    /// Meilleur choix pour un champion à un rôle. Si le rôle a trop peu de parties
    /// (ou est inconnu), on regroupe tous les rôles du champion.
    pub fn recommander(&self, champion: u16, role: Option<&str>) -> Option<Recommandation> {
        let total = |m: &HashMap<PageRunes, Compteur>| m.values().map(|c| c.parties).sum::<u32>();
        let mut role_utilise = role.map(str::to_string);
        let pages = match role.map(|r| regrouper(&self.pages, champion, Some(r))) {
            Some(m) if total(&m) >= MIN_PARTIES_ROLE => m,
            _ => {
                role_utilise = None;
                regrouper(&self.pages, champion, None)
            }
        };
        let parties = total(&pages);
        if parties == 0 {
            return None;
        }
        let victoires: u32 = pages.values().map(|c| c.victoires).sum();
        let moyenne = victoires as f64 / parties as f64;
        const POIDS: f64 = 200.0;
        let choisir = |m: Vec<(PageRunes, Compteur)>| {
            m.into_iter().max_by(|a, b| a.1.prudent(moyenne, POIDS).total_cmp(&b.1.prudent(moyenne, POIDS)))
        };
        // On ne garde que les pages jouées dans au moins 10 % des parties.
        let candidates: Vec<_> = pages.iter().filter(|(_, c)| c.parties * 10 >= parties).map(|(p, c)| (*p, *c)).collect();
        let (page, stats_page) = choisir(candidates)?;
        let plus_jouee = pages.iter().max_by_key(|(_, c)| c.parties).map(|(p, c)| (*p, *c))?;

        let sorts = regrouper(&self.sorts, champion, role_utilise.as_deref());
        let total_sorts: u32 = sorts.values().map(|c| c.parties).sum();
        let moyenne_sorts = sorts.values().map(|c| c.victoires).sum::<u32>() as f64 / total_sorts.max(1) as f64;
        let sorts = sorts
            .into_iter()
            .filter(|(_, c)| c.parties * 10 >= total_sorts)
            .max_by(|a, b| a.1.prudent(moyenne_sorts, POIDS).total_cmp(&b.1.prudent(moyenne_sorts, POIDS)));

        Some(Recommandation {
            page,
            stats: stats_page,
            pick_rate: stats_page.parties as f32 * 100.0 / parties as f32,
            plus_jouee,
            parties_champion: parties,
            sorts,
            role: role_utilise,
        })
    }

    // ---------- Matchups de lane ----------

    /// Mon résultat (champion à ce rôle) contre un adversaire précis.
    /// `role = None` : tous rôles confondus.
    pub fn matchup(&self, champion: u16, role: Option<&str>, adversaire: u16) -> Option<Compteur> {
        regrouper(&self.matchups, champion, role).get(&adversaire).copied().filter(|c| c.parties > 0)
    }

    /// Meilleurs et pires matchups d'un champion à un rôle : `nombre` de chaque,
    /// seulement les adversaires vus au moins `min_parties` fois.
    /// Tri par winrate lissé vers le winrate global du champion (comme les runes).
    pub fn matchups(&self, champion: u16, role: &str, min_parties: u32, nombre: usize) -> (Vec<LigneMatchup>, Vec<LigneMatchup>) {
        let lignes: Vec<(u16, Compteur)> = regrouper(&self.matchups, champion, Some(role)).into_iter().collect();
        let tries = trier_lisse(lignes, min_parties, Some(champion));
        // Avec peu d'adversaires, un même matchup ne doit pas être à la fois
        // « meilleur » et « pire » : les meilleurs prennent au plus la moitié haute.
        let k = nombre.min(tries.len().div_ceil(2));
        let pires: Vec<LigneMatchup> = tries[k..].iter().rev().take(nombre).copied().collect();
        (tries[..k].to_vec(), pires)
    }

    /// Contre-picks : les champions de `role` qui gagnent le plus contre `adversaire`
    /// (qui joue ce même rôle). Astuce : chaque duel est compté des DEUX côtés, donc
    /// on n'a besoin que des lignes de l'adversaire (lecture filtrée sur lui seul).
    pub fn contre_picks(&self, adversaire: u16, role: &str, min_parties: u32, nombre: usize) -> Vec<LigneMatchup> {
        let lignes: Vec<(u16, Compteur)> = regrouper(&self.matchups, adversaire, Some(role))
            .into_iter()
            .map(|(moi, c)| (moi, c.inverse()))
            .collect();
        let mut tries = trier_lisse(lignes, min_parties, Some(adversaire));
        tries.truncate(nombre);
        tries
    }

    /// Rôles joués par un champion, du plus joué au moins joué (lignes `C`).
    pub fn roles_joues(&self, champion: u16) -> Vec<(&'static str, u32)> {
        let mut v: Vec<(&'static str, u32)> =
            self.totaux.iter().filter(|((c, _), _)| *c == champion).map(|((_, r), n)| (*r, n.parties)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        v
    }

    /// Adversaire probable de ma lane en champ select (le client ne donne pas les
    /// rôles ennemis) : l'ennemi dont le rôle le plus joué (d'après les stats ; à
    /// défaut `role_par_defaut`, ex. le premier rôle de `compo::profil`) est le mien.
    /// S'il y en a plusieurs, celui qui joue le plus souvent ce rôle.
    pub fn adversaire_probable(
        &self,
        ennemis: &[u16],
        role: &str,
        role_par_defaut: &dyn Fn(u16) -> Option<&'static str>,
    ) -> Option<u16> {
        ennemis
            .iter()
            .filter_map(|&e| {
                let roles = self.roles_joues(e);
                let total: u32 = roles.iter().map(|r| r.1).sum();
                if total >= MIN_PARTIES_ROLE {
                    // Part du rôle principal (0 à 1), seulement s'il est le mien.
                    let (principal, n) = roles[0];
                    (principal == role).then_some((e, n as f32 / total as f32))
                } else {
                    // Sans stats, l'avis par défaut compte comme une part de 50 %.
                    (role_par_defaut(e) == Some(role)).then_some((e, 0.5))
                }
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(e, _)| e)
    }

    // ---------- Objets ----------

    /// Objets conseillés : les légendaires et bottes les plus présents en fin de
    /// partie. Même repli que les runes si le rôle a trop peu de parties.
    pub fn conseil_objets(&self, champion: u16, role: Option<&str>) -> Option<ConseilObjets> {
        let total = |r: Option<&str>| -> u32 {
            self.totaux.iter().filter(|((c, ro), _)| *c == champion && r.is_none_or(|x| x == *ro)).map(|(_, n)| n.parties).sum()
        };
        let role = role.filter(|r| total(Some(r)) >= MIN_PARTIES_ROLE);
        let parties = total(role);
        // Du plus fréquent au moins fréquent ; on écarte ce qui est construit dans moins de 5 % des parties.
        let frequents = |table: &HashMap<Cle, HashMap<u32, Compteur>>| {
            let mut v: Vec<(u32, Compteur)> =
                regrouper(table, champion, role).into_iter().filter(|(_, c)| c.parties * 20 >= parties).collect();
            v.sort_by(|a, b| b.1.parties.cmp(&a.1.parties).then(a.0.cmp(&b.0)));
            v
        };
        let mut legendaires = frequents(&self.legendaires);
        let mut bottes = frequents(&self.bottes);
        if parties == 0 || (legendaires.is_empty() && bottes.is_empty()) {
            return None;
        }
        bottes.truncate(2);
        let situationnels: Vec<(u32, Compteur)> = legendaires.iter().skip(3).take(5).copied().collect();
        legendaires.truncate(3);
        Some(ConseilObjets { core: legendaires, situationnels, bottes, parties, role: role.map(String::from) })
    }

    // ---------- Tier list ----------

    /// Tier list à partir des totaux et des bans (lignes `C` et `B`).
    /// `role = None` : tous les rôles. Un champion doit avoir au moins 20 parties
    /// et 0,5 % de pick rate au rôle.
    pub fn tier_list(&self, role: Option<&str>) -> TierList {
        let parties = self.parties.max(1);
        let minimum = (parties / 200).max(20);
        let mut lignes: Vec<LigneTier> = self
            .totaux
            .iter()
            .filter(|((_, r), c)| role.is_none_or(|x| x == *r) && c.parties >= minimum)
            .map(|(&(champion, role), &c)| LigneTier {
                champion,
                role,
                stats: c,
                pick_rate: c.parties as f32 * 100.0 / parties as f32,
                ban_rate: self.bans.get(&champion).copied().unwrap_or(0) as f32 * 100.0 / parties as f32,
                score: (c.prudent(0.5, POIDS_TIER) * 100.0) as f32,
            })
            .collect();
        let rang = |r: &str| ROLES.iter().position(|x| *x == r).unwrap_or(ROLES.len());
        lignes.sort_by(|a, b| rang(a.role).cmp(&rang(b.role)).then(b.score.total_cmp(&a.score)).then(a.champion.cmp(&b.champion)));
        TierList { patch: self.patch.clone(), parties: self.parties, lignes }
    }
}

/// Trie des duels par winrate lissé (du meilleur au pire). La moyenne vers laquelle
/// on lisse = le winrate de tous ces duels réunis. `exclure` : le miroir (X contre X).
fn trier_lisse(lignes: Vec<(u16, Compteur)>, min_parties: u32, exclure: Option<u16>) -> Vec<LigneMatchup> {
    let mut tout = Compteur::default();
    for (_, c) in &lignes {
        tout.fusionner(*c);
    }
    let moyenne = if tout.parties == 0 { 0.5 } else { tout.victoires as f64 / tout.parties as f64 };
    let mut v: Vec<LigneMatchup> = lignes
        .into_iter()
        .filter(|(ch, c)| c.parties >= min_parties.max(1) && Some(*ch) != exclure)
        .map(|(champion, stats)| LigneMatchup { champion, stats, score: (stats.prudent(moyenne, POIDS_MATCHUP) * 100.0) as f32 })
        .collect();
    v.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.champion.cmp(&b.champion)));
    v
}

#[derive(Debug, Clone)]
pub struct Recommandation {
    pub page: PageRunes,
    pub stats: Compteur,
    pub pick_rate: f32,
    /// Page la plus jouée (souvent la même), pour comparaison.
    pub plus_jouee: (PageRunes, Compteur),
    pub parties_champion: u32,
    /// Paire de sorts conseillée (ids rangés), avec ses stats.
    pub sorts: Option<((u16, u16), Compteur)>,
    /// Rôle des stats (None = tous rôles confondus, faute de données).
    pub role: Option<String>,
}

// ---------- Fichiers ----------

pub fn dossier() -> PathBuf {
    crate::stockage::dossier_cache("stats")
}

pub fn chemin_patch(patch: &str) -> PathBuf {
    dossier().join(format!("stats-{patch}.tsv"))
}

/// "16.23.1" -> "16.23" (les parties indiquent "16.23.xxx.yyyy").
pub fn patch_court(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Appelle `f` sur chaque ligne du fichier, avec UN seul tampon réutilisé :
/// la RAM utilisée ne dépend pas de la taille du fichier.
fn lire_lignes(chemin: &Path, mut f: impl FnMut(&str)) -> Option<()> {
    let mut lecteur = std::io::BufReader::new(std::fs::File::open(chemin).ok()?);
    let mut ligne = String::new();
    loop {
        ligne.clear();
        match lecteur.read_line(&mut ligne) {
            Ok(0) => return Some(()),
            Ok(_) => f(ligne.trim_end_matches(['\n', '\r'])),
            Err(_) => return None,
        }
    }
}

/// Recommandation pour un champion, en ne lisant que ses lignes du fichier.
pub fn recommander_depuis_fichier(chemin: &Path, champion: u16, role: Option<&str>) -> Option<Recommandation> {
    Stats::charger_filtre(chemin, &[champion])?.recommander(champion, role)
}

/// Tier list en parcourant le fichier en flux : seules les lignes `C` et `B`
/// (une par champion et par rôle) sont gardées en RAM.
pub fn tier_list_depuis_fichier(chemin: &Path, role: Option<&str>) -> Option<TierList> {
    let mut s = Stats::default();
    lire_lignes(chemin, |l| s.lire_ligne(l, &|t, _| t == "C" || t == "B"))?;
    Some(s.tier_list(role))
}

/// Commande console `stat-traque tierlist [rôle]`.
pub fn commande_tierlist(role: Option<&str>) {
    let role = match role.map(|r| (r, role_depuis_texte(r))) {
        Some((texte, None)) => {
            println!("Rôle inconnu : {texte} (top, jungle, mid, adc, support).");
            return;
        }
        Some((_, r)) => r,
        None => None,
    };
    let Some(d) = crate::ddragon::charger() else {
        println!("Data Dragon indisponible (pas d'internet et pas de cache).");
        return;
    };
    let patch = patch_court(&d.version);
    let Some(t) = trouver_fichier(&patch).and_then(|f| tier_list_depuis_fichier(&f, role)) else {
        println!("Pas de stats pour le patch {patch} (lance `stat-traque collecter 30`).");
        return;
    };
    if t.lignes.is_empty() {
        println!("Pas assez de parties (ou fichier d'un ancien format : relance la collecte).");
        return;
    }
    println!("Tier list du patch {} ({} parties)", t.patch, t.parties);
    for r in ROLES.iter().filter(|r| role.is_none_or(|x| x == **r)) {
        println!("\n--- {r} ---\n  #  {:<16} {:>8} {:>7} {:>7} {:>8}", "champion", "victoire", "pick", "ban", "parties");
        for (i, l) in t.du_role(r).take(if role.is_some() { 50 } else { 15 }).enumerate() {
            println!(
                "{:>3}  {:<16} {:>6.1} % {:>5.1} % {:>5.1} % {:>8}",
                i + 1, d.nom_champion(l.champion), l.stats.winrate(), l.pick_rate, l.ban_rate, l.stats.parties
            );
        }
    }
}

/// Où trouver le fichier de stats : à côté de l'exe d'abord (paquet partagé entre
/// amis), sinon dans le cache (collecte locale ou téléchargement).
pub fn trouver_fichier(patch: &str) -> Option<PathBuf> {
    let nom = format!("stats-{patch}.tsv");
    let pres_exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(&nom)));
    pres_exe.into_iter().chain(std::iter::once(chemin_patch(patch))).find(|p| p.exists())
}

/// Adresse du fichier publié chaque jour par la tâche GitHub (voir .github/workflows/stats.yml).
const URL_STATS: &str = "https://github.com/loiccotte/stat-traque/releases/download/stats";

/// Télécharge le fichier de stats du patch s'il manque ou date de plus de 24 h.
/// Appelé en arrière-plan au démarrage. Si une collecte locale existe, on la garde
/// et on y ajoute les comptes publiés.
pub fn mettre_a_jour(patch: &str) {
    let chemin = chemin_patch(patch);
    let recent = std::fs::metadata(&chemin)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < 24 * 3600);
    if recent {
        return;
    }
    let url = format!("{URL_STATS}/stats-{patch}.tsv");
    match crate::http::internet().get(&url).call().ok().and_then(|r| r.into_string().ok()) {
        Some(texte) if texte.starts_with("#patch") => {
            let _ = std::fs::write(&chemin, texte);
            crate::log!("Stats du patch {patch} mises à jour.");
        }
        _ => crate::log!("Pas encore de stats publiées pour le patch {patch}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn participant(champion: u64, role: &str, win: bool, keystone: u64) -> Value {
        json!({"championId": champion, "teamPosition": role, "win": win,
               "summoner1Id": 14, "summoner2Id": 4,
               "perks": {"statPerks": {"offense": 5008, "flex": 5008, "defense": 5001},
                         "styles": [{"description": "primaryStyle", "style": 8100,
                                     "selections": [{"perk": keystone}, {"perk": 8143}, {"perk": 8138}, {"perk": 8105}]},
                                    {"description": "subStyle", "style": 8300,
                                     "selections": [{"perk": 8304}, {"perk": 8347}]}]}})
    }

    fn partie(participants: Vec<Value>) -> Value {
        json!({"info": {"gameVersion": "16.23.712.3456", "participants": participants}})
    }

    /// Aucun objet classé (tests des runes).
    fn sans_objets(_: u32) -> Option<GenreObjet> {
        None
    }

    /// Faux Data Dragon : 3089, 3157, 4645, 3135, 3165 légendaires ; 3020 et 3158 bottes.
    fn genres(id: u32) -> Option<GenreObjet> {
        match id {
            3089 | 3157 | 4645 | 3135 | 3165 => Some(GenreObjet::Legendaire),
            3020 | 3158 => Some(GenreObjet::Bottes),
            _ => None,
        }
    }

    /// Un participant complet : équipe, objets.
    fn joueur(champion: u64, role: &str, equipe: u64, win: bool, objets: &[u64]) -> Value {
        let mut p = participant(champion, role, win, 8112);
        p["teamId"] = json!(equipe);
        for (i, o) in objets.iter().enumerate() {
            p[format!("item{i}")] = json!(o);
        }
        p
    }

    /// Partie mid Zed (238, bleu) contre Ahri (103, rouge), avec un top de chaque côté.
    fn partie_mid(zed_gagne: bool, objets_zed: &[u64]) -> Value {
        let mut p = partie(vec![
            joueur(238, "MIDDLE", 100, zed_gagne, objets_zed),
            joueur(54, "TOP", 100, zed_gagne, &[]),
            joueur(103, "MIDDLE", 200, !zed_gagne, &[3089, 3020, 2003]),
            joueur(266, "TOP", 200, !zed_gagne, &[]),
        ]);
        p["info"]["teams"] = json!([
            {"teamId": 100, "bans": [{"championId": 157}, {"championId": -1}]},
            {"teamId": 200, "bans": [{"championId": 157}, {"championId": 555}]}
        ]);
        p
    }

    #[test]
    fn compte_recommande_et_relit() {
        let mut s = Stats::nouveau("16.23");
        // Électrocution (8112) : 40 parties, 24 victoires. Premier coup (8369 ici en faux) : 4 parties, 4 victoires.
        for i in 0..40 {
            assert!(s.ajouter_partie(&partie(vec![participant(238, "MIDDLE", i < 24, 8112)]), &sans_objets));
        }
        for _ in 0..4 {
            s.ajouter_partie(&partie(vec![participant(238, "MIDDLE", true, 9923)]), &sans_objets);
        }
        let r = s.recommander(238, Some("middle")).unwrap();
        // Page jouée dans 9 % des parties seulement : écartée malgré 4 victoires sur 4.
        assert_eq!(r.page.perks[0], 8112);
        assert_eq!(r.page.perks, [8112, 8143, 8138, 8105, 8304, 8347, 5008, 5008, 5001]);
        assert_eq!((r.page.principal, r.page.secondaire), (8100, 8300));
        assert_eq!(r.sorts.unwrap().0, (4, 14));
        assert_eq!(r.role.as_deref(), Some("middle"));

        // Aller-retour texte, en ne lisant qu'un champion.
        let relu = Stats::depuis_texte(&s.vers_texte(1), Some(238));
        assert_eq!(relu.patch, "16.23");
        assert_eq!(relu.nombre_parties_vues(), 44);
        assert_eq!(relu.recommander(238, Some("middle")).unwrap().page, r.page);
        assert!(Stats::depuis_texte(&s.vers_texte(1), Some(1)).recommander(238, None).is_none());
    }

    #[test]
    fn ignore_les_autres_patchs_et_regroupe_les_roles() {
        let mut s = Stats::nouveau("16.23");
        let mut vieille = partie(vec![participant(238, "MIDDLE", true, 8112)]);
        vieille["info"]["gameVersion"] = json!("16.22.1.1");
        assert!(!s.ajouter_partie(&vieille, &sans_objets));
        for _ in 0..5 {
            s.ajouter_partie(&partie(vec![participant(238, "TOP", true, 8112)]), &sans_objets);
        }
        // Rôle jungle demandé, aucune donnée : repli sur tous les rôles.
        let r = s.recommander(238, Some("jungle")).unwrap();
        assert!(r.role.is_none());
        assert_eq!(r.parties_champion, 5);
    }

    #[test]
    fn lit_l_ancien_format_sans_lignes_c() {
        // Fichier écrit par l'ancienne version : seulement #patch, P et S.
        let ancien = "#patch\t16.23\n\
            P\t238\tmiddle\t8100\t8300\t8112,8143,8138,8105,8304,8347,5008,5008,5001\t40\t24\n\
            S\t238\tmiddle\t4\t14\t40\t24";
        let s = Stats::depuis_texte(ancien, None);
        assert_eq!(s.roles_joues(238), vec![("middle", 40)]);
        assert_eq!(s.nombre_parties_vues(), 4);
        assert_eq!(s.recommander(238, Some("middle")).unwrap().stats.parties, 40);
        assert!(s.conseil_objets(238, Some("middle")).is_none());
        assert!(s.matchup(238, Some("middle"), 103).is_none());
    }

    #[test]
    fn matchups_de_lane_et_contre_picks() {
        let mut s = Stats::nouveau("16.23");
        // Zed gagne 30 parties sur 40 contre Ahri.
        for i in 0..40 {
            s.ajouter_partie(&partie_mid(i < 30, &[]), &sans_objets);
        }
        let zed = s.matchup(238, Some("middle"), 103).unwrap();
        assert_eq!((zed.parties, zed.victoires), (40, 30));
        let ahri = s.matchup(103, Some("middle"), 238).unwrap();
        assert_eq!((ahri.parties, ahri.victoires), (40, 10));
        // Pas de matchup entre rôles différents.
        assert!(s.matchup(238, Some("middle"), 266).is_none());
        assert_eq!(s.matchup(54, Some("top"), 266).unwrap().parties, 40);

        // Un 2e adversaire pour Ahri : Syndra (134) qui perd 15 fois sur 20 contre elle.
        for i in 0..20 {
            let mut p = partie_mid(true, &[]);
            p["info"]["participants"][0]["championId"] = json!(134);
            p["info"]["participants"][0]["win"] = json!(i >= 15);
            p["info"]["participants"][2]["win"] = json!(i < 15);
            s.ajouter_partie(&p, &sans_objets);
        }
        // Contre-picks d'Ahri : Zed (75 %) avant Syndra (25 %).
        let cp = s.contre_picks(103, "middle", 10, 5);
        assert_eq!(cp.iter().map(|l| l.champion).collect::<Vec<_>>(), vec![238, 134]);
        assert_eq!(cp[0].stats, Compteur { parties: 40, victoires: 30 });
        assert!(cp[0].score < 75.0 && cp[0].score > 50.0, "lissé : {}", cp[0].score);
        // Minimum de parties : Syndra (20 parties) écartée.
        assert_eq!(s.contre_picks(103, "middle", 25, 5).len(), 1);
        // Meilleurs / pires matchups d'Ahri.
        let (meilleurs, pires) = s.matchups(103, "middle", 10, 1);
        assert_eq!((meilleurs[0].champion, pires[0].champion), (134, 238));
        // Un seul adversaire connu : il n'est pas à la fois meilleur et pire.
        let (meilleurs, pires) = s.matchups(238, "middle", 10, 3);
        assert_eq!((meilleurs.len(), pires.len()), (1, 0));

        // Aller-retour texte + lecture filtrée sur l'adversaire SEUL : suffit aux contre-picks.
        let texte = s.vers_texte(1);
        let relu = Stats::depuis_texte(&texte, Some(103));
        assert_eq!(relu.contre_picks(103, "middle", 10, 5), cp);
        assert!(relu.matchup(238, Some("middle"), 103).is_none(), "lignes de Zed non lues");
    }

    #[test]
    fn adversaire_probable_d_apres_les_roles() {
        let mut s = Stats::nouveau("16.23");
        for _ in 0..40 {
            s.ajouter_partie(&partie_mid(true, &[]), &sans_objets);
        }
        // Ahri (103) joue mid d'après les stats ; Garen (86) inconnu des stats -> profil par défaut.
        let defaut = |c: u16| match c {
            86 => Some("top"),
            99 => Some("middle"),
            _ => None,
        };
        assert_eq!(s.adversaire_probable(&[86, 103], "middle", &defaut), Some(103));
        assert_eq!(s.adversaire_probable(&[86, 103], "top", &defaut), Some(86));
        // Ahri avec stats (100 % mid) passe devant un mid « par défaut ».
        assert_eq!(s.adversaire_probable(&[99, 103], "middle", &defaut), Some(103));
        assert_eq!(s.adversaire_probable(&[99], "middle", &defaut), Some(99));
        assert_eq!(s.adversaire_probable(&[86, 103], "jungle", &defaut), None);
    }

    #[test]
    fn objets_core_et_bottes() {
        let mut s = Stats::nouveau("16.23");
        // 40 parties de Zed : Coiffe (3089) toujours (doublée en case 2 : comptée une fois),
        // 3157 30 fois, 4645 20 fois, 3135 10 fois, 3165 1 fois (< 5 %), bottes 3020 ou 3158,
        // un composant (1058) jamais compté.
        for i in 0..40u64 {
            let mut objets = vec![3089, 3089, if i < 25 { 3020 } else { 3158 }];
            if i < 30 {
                objets.push(3157)
            }
            if i < 20 {
                objets.push(4645)
            }
            if i < 10 {
                objets.push(3135)
            }
            if i == 39 {
                objets.extend([3165, 1058]);
            }
            s.ajouter_partie(&partie_mid(i % 2 == 0, &objets), &genres);
        }
        let c = s.conseil_objets(238, Some("middle")).unwrap();
        assert_eq!(c.parties, 40);
        assert_eq!(c.core.iter().map(|o| o.0).collect::<Vec<_>>(), vec![3089, 3157, 4645]);
        assert_eq!(c.core[0].1, Compteur { parties: 40, victoires: 20 });
        assert_eq!(c.frequence(&c.core[1].1), 75.0);
        assert_eq!(c.situationnels.iter().map(|o| o.0).collect::<Vec<_>>(), vec![3135]);
        assert_eq!(c.bottes.iter().map(|o| o.0).collect::<Vec<_>>(), vec![3020, 3158]);
        assert_eq!(c.role.as_deref(), Some("middle"));
        let blocs = c.blocs();
        assert_eq!(blocs.len(), 3);
        assert_eq!(blocs[0], ("Core (40 parties, middle)".to_string(), vec![3089, 3157, 4645]));
        assert_eq!(blocs[1].1, vec![3020, 3158]);

        // Aller-retour texte : mêmes conseils.
        let relu = Stats::depuis_texte(&s.vers_texte(1), Some(238));
        assert_eq!(relu.conseil_objets(238, Some("middle")), Some(c));
        // Rôle peu joué : repli sur tous les rôles.
        assert!(s.conseil_objets(238, Some("top")).unwrap().role.is_none());
    }

    #[test]
    fn tier_list_avec_bans() {
        let mut s = Stats::nouveau("16.23");
        for i in 0..40 {
            s.ajouter_partie(&partie_mid(i < 30, &[]), &sans_objets);
        }
        let t = s.tier_list(Some("middle"));
        assert_eq!(t.parties, 40);
        assert_eq!(t.lignes.iter().map(|l| l.champion).collect::<Vec<_>>(), vec![238, 103]);
        let zed = t.lignes[0];
        assert_eq!(zed.pick_rate, 100.0);
        // Zed n'est jamais banni ; 157 (Yasuo) banni par les deux équipes = 1 fois par partie.
        assert_eq!(zed.ban_rate, 0.0);
        assert_eq!(s.bans.get(&157), Some(&40));
        assert_eq!(s.bans.get(&555), Some(&40));
        assert!(zed.score > 50.0 && zed.score < 75.0);
        // Tous rôles : mid et top, dans l'ordre des rôles.
        let tout = s.tier_list(None);
        assert_eq!(tout.lignes.len(), 4);
        assert_eq!(tout.lignes[0].role, "top");
        assert_eq!(tout.du_role("middle").count(), 2);

        // Lecture en flux depuis un fichier : mêmes résultats.
        let chemin = std::env::temp_dir().join(format!("stat-traque-test-tier-{}.tsv", std::process::id()));
        std::fs::write(&chemin, s.vers_texte(1)).unwrap();
        let lu = tier_list_depuis_fichier(&chemin, Some("middle")).unwrap();
        assert_eq!(lu, t);
        // Lecture filtrée du fichier (champ select).
        let filtre = Stats::charger_filtre(&chemin, &[103]).unwrap();
        assert_eq!(filtre.contre_picks(103, "middle", 1, 3)[0].champion, 238);
        assert!(filtre.memoire_estimee() > 0);
        let _ = std::fs::remove_file(&chemin);
    }

    #[test]
    fn roles_tapes_par_l_utilisateur() {
        assert_eq!(role_depuis_texte("MID"), Some("middle"));
        assert_eq!(role_depuis_texte("adc"), Some("bottom"));
        assert_eq!(role_depuis_texte("support"), Some("utility"));
        assert_eq!(role_depuis_texte("nimporte"), None);
    }
}
