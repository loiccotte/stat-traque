//! Statistiques de runes et de sorts par champion et par rôle, sur le patch actuel.
//!
//! D'où viennent les chiffres ? Blitz a des serveurs qui analysent des millions de
//! parties. Nous, on fait pareil SANS serveur :
//!  - `stat-traque collecter` lit des parties classées Master+ via l'API Riot
//!    (voir collecte.rs) et les compte ici ;
//!  - une tâche GitHub automatique le fait chaque jour et publie le fichier ;
//!  - l'app télécharge ce fichier (quelques centaines de Ko) une fois par jour.
//!
//! Optimisation : l'app ne charge PAS tout le fichier en RAM. Quand tu verrouilles
//! un champion, elle lit le fichier et ne garde que les lignes de CE champion.
//!
//! Format (TSV, une ligne par page / paire de sorts) :
//!   #patch<TAB>16.23
//!   P<TAB>champion<TAB>rôle<TAB>style1<TAB>style2<TAB>9 perks (virgules)<TAB>parties<TAB>victoires
//!   S<TAB>champion<TAB>rôle<TAB>sort1<TAB>sort2<TAB>parties<TAB>victoires

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Rôles, au format du client LoL (`assignedPosition`).
pub const ROLES: [&str; 5] = ["top", "jungle", "middle", "bottom", "utility"];

/// match-v5 écrit "TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY".
fn role_depuis_match(position: &str) -> Option<&'static str> {
    ROLES.iter().copied().find(|r| r.eq_ignore_ascii_case(position))
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
}

/// Clé : (champion, rôle).
type Cle = (u16, &'static str);

#[derive(Debug, Default)]
pub struct Stats {
    pub patch: String,
    pages: HashMap<Cle, HashMap<PageRunes, Compteur>>,
    /// Paires de sorts rangées (petit id d'abord) : Flash sur D ou F = même choix.
    sorts: HashMap<Cle, HashMap<(u16, u16), Compteur>>,
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

impl Stats {
    pub fn nouveau(patch: &str) -> Self {
        Self { patch: patch.to_string(), ..Default::default() }
    }

    /// Compte les 10 participants d'une partie match-v5. Renvoie false si la
    /// partie n'est pas du bon patch (ou illisible).
    pub fn ajouter_partie(&mut self, partie: &Value) -> bool {
        let info = &partie["info"];
        let version = info["gameVersion"].as_str().unwrap_or("");
        if !version.starts_with(&format!("{}.", self.patch)) {
            return false;
        }
        let Some(participants) = info["participants"].as_array() else { return false };
        for p in participants {
            let (Some(champion), Some(role)) =
                (p["championId"].as_u64(), p["teamPosition"].as_str().and_then(role_depuis_match))
            else {
                continue;
            };
            let cle = (champion as u16, role);
            let victoire = p["win"] == true;
            if let Some(page) = page_depuis_match(&p["perks"]) {
                self.pages.entry(cle).or_default().entry(page).or_default().ajouter(victoire);
            }
            if let (Some(a), Some(b)) = (p["summoner1Id"].as_u64(), p["summoner2Id"].as_u64()) {
                let paire = (a.min(b) as u16, a.max(b) as u16);
                self.sorts.entry(cle).or_default().entry(paire).or_default().ajouter(victoire);
            }
        }
        true
    }

    /// Texte du fichier. `min_parties` : on n'écrit pas les combinaisons trop rares.
    pub fn vers_texte(&self, min_parties: u32) -> String {
        let mut lignes = vec![format!("#patch\t{}", self.patch)];
        let mut cles: Vec<&Cle> = self.pages.keys().chain(self.sorts.keys()).collect();
        cles.sort();
        cles.dedup();
        for cle in cles {
            for (page, c) in self.pages.get(cle).into_iter().flatten().filter(|(_, c)| c.parties >= min_parties) {
                let perks: Vec<String> = page.perks.iter().map(u16::to_string).collect();
                lignes.push(format!(
                    "P\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    cle.0, cle.1, page.principal, page.secondaire, perks.join(","), c.parties, c.victoires
                ));
            }
            for ((a, b), c) in self.sorts.get(cle).into_iter().flatten().filter(|(_, c)| c.parties >= min_parties) {
                lignes.push(format!("S\t{}\t{}\t{a}\t{b}\t{}\t{}", cle.0, cle.1, c.parties, c.victoires));
            }
        }
        lignes.join("\n")
    }

    /// Lit un fichier. `seulement` : ne garder qu'un champion (lecture économe en RAM).
    pub fn depuis_texte(texte: &str, seulement: Option<u16>) -> Self {
        let mut s = Stats::default();
        for ligne in texte.lines() {
            let c: Vec<&str> = ligne.split('\t').collect();
            if c.first() == Some(&"#patch") {
                s.patch = c.get(1).unwrap_or(&"").to_string();
                continue;
            }
            let Some(champion) = c.get(1).and_then(|x| x.parse::<u16>().ok()) else { continue };
            if seulement.is_some_and(|id| id != champion) {
                continue;
            }
            let Some(role) = c.get(2).and_then(|r| ROLES.iter().copied().find(|x| x == r)) else { continue };
            let n = |i: usize| c.get(i).and_then(|x| x.parse::<u32>().ok());
            match (c[0], c.len()) {
                ("P", 8) => {
                    let perks: Vec<u16> = c[5].split(',').filter_map(|p| p.parse().ok()).collect();
                    let (Some(p1), Some(p2), Ok(perks), Some(parties), Some(victoires)) =
                        (n(3), n(4), <[u16; 9]>::try_from(perks), n(6), n(7))
                    else {
                        continue;
                    };
                    let page = PageRunes { principal: p1 as u16, secondaire: p2 as u16, perks };
                    s.pages.entry((champion, role)).or_default().entry(page).or_default().fusionner(Compteur { parties, victoires });
                }
                ("S", 7) => {
                    if let (Some(a), Some(b), Some(parties), Some(victoires)) = (n(3), n(4), n(5), n(6)) {
                        s.sorts.entry((champion, role)).or_default().entry((a as u16, b as u16)).or_default().fusionner(Compteur { parties, victoires });
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn nombre_parties_vues(&self) -> u32 {
        // Chaque partie compte 10 participants.
        self.pages.values().flat_map(|m| m.values()).map(|c| c.parties).sum::<u32>() / 10
    }

    /// Meilleur choix pour un champion à un rôle. Si le rôle a trop peu de parties
    /// (ou est inconnu), on regroupe tous les rôles du champion.
    pub fn recommander(&self, champion: u16, role: Option<&str>) -> Option<Recommandation> {
        const MIN_PARTIES: u32 = 30;
        let du_role = |table: &HashMap<Cle, HashMap<PageRunes, Compteur>>| {
            role.and_then(|r| table.iter().find(|((c, ro), _)| *c == champion && *ro == r).map(|(_, m)| m.clone()))
        };
        let total = |m: &HashMap<PageRunes, Compteur>| m.values().map(|c| c.parties).sum::<u32>();
        let mut role_utilise = role.map(str::to_string);
        let pages = match du_role(&self.pages) {
            Some(m) if total(&m) >= MIN_PARTIES => m,
            _ => {
                role_utilise = None;
                let mut tout: HashMap<PageRunes, Compteur> = HashMap::new();
                for ((c, _), m) in &self.pages {
                    if *c == champion {
                        for (p, n) in m {
                            tout.entry(*p).or_default().fusionner(*n);
                        }
                    }
                }
                tout
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

        let mut sorts: HashMap<(u16, u16), Compteur> = HashMap::new();
        for ((c, r), m) in &self.sorts {
            if *c == champion && role_utilise.as_deref().is_none_or(|ru| ru == *r) {
                for (p, n) in m {
                    sorts.entry(*p).or_default().fusionner(*n);
                }
            }
        }
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

/// Recommandation pour un champion, en ne lisant que ses lignes du fichier.
pub fn recommander_depuis_fichier(chemin: &Path, champion: u16, role: Option<&str>) -> Option<Recommandation> {
    let texte = std::fs::read_to_string(chemin).ok()?;
    Stats::depuis_texte(&texte, Some(champion)).recommander(champion, role)
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

    #[test]
    fn compte_recommande_et_relit() {
        let mut s = Stats::nouveau("16.23");
        // Électrocution (8112) : 40 parties, 24 victoires. Premier coup (8369 ici en faux) : 4 parties, 4 victoires.
        for i in 0..40 {
            assert!(s.ajouter_partie(&partie(vec![participant(238, "MIDDLE", i < 24, 8112)])));
        }
        for _ in 0..4 {
            s.ajouter_partie(&partie(vec![participant(238, "MIDDLE", true, 9923)]));
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
        assert_eq!(relu.recommander(238, Some("middle")).unwrap().page, r.page);
        assert!(Stats::depuis_texte(&s.vers_texte(1), Some(1)).recommander(238, None).is_none());
    }

    #[test]
    fn ignore_les_autres_patchs_et_regroupe_les_roles() {
        let mut s = Stats::nouveau("16.23");
        let mut vieille = partie(vec![participant(238, "MIDDLE", true, 8112)]);
        vieille["info"]["gameVersion"] = json!("16.22.1.1");
        assert!(!s.ajouter_partie(&vieille));
        for _ in 0..5 {
            s.ajouter_partie(&partie(vec![participant(238, "TOP", true, 8112)]));
        }
        // Rôle jungle demandé, aucune donnée : repli sur tous les rôles.
        let r = s.recommander(238, Some("jungle")).unwrap();
        assert!(r.role.is_none());
        assert_eq!(r.parties_champion, 5);
    }
}
