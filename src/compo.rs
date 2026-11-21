//! Analyse de composition d'équipe (champ select) : style de la compo, dégâts AP/AD,
//! frontline, contrôle, courbe de puissance, alertes, rôle de chaque champion,
//! plan de jeu contre la compo ennemie et picks qui complètent la compo.
//!
//! Les profils des champions sont dans `data/profils_champions.tsv`, INCLUS dans
//! l'exe à la compilation (~10 Ko) et lus une seule fois, sans copie : les textes
//! pointent directement dans le fichier inclus.

use std::collections::HashMap;
use std::sync::OnceLock;

const PROFILS: &str = include_str!("../data/profils_champions.tsv");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profil {
    pub id: &'static str,
    pub roles: &'static str,
    /// Part de dégâts magiques en %.
    pub ap: u8,
    pub engage: u8,
    pub poke: u8,
    pub allin: u8,
    pub pick: u8,
    pub peel: u8,
    pub cc: u8,
    pub frontline: u8,
    pub split: u8,
    pub teamfight: u8,
    pub early: u8,
    pub mid: u8,
    pub late: u8,
    pub distance: bool,
}

impl Profil {
    pub fn joue(&self, role: &str) -> bool {
        self.roles.split(',').any(|r| r == role)
    }
}

fn lire_ligne(ligne: &'static str) -> Option<Profil> {
    let c: Vec<&'static str> = ligne.split('\t').collect();
    if c.len() != 16 {
        return None;
    }
    let n = |i: usize| c[i].trim().parse::<u8>().ok();
    Some(Profil {
        id: c[0],
        roles: c[1],
        ap: n(2)?,
        engage: n(3)?,
        poke: n(4)?,
        allin: n(5)?,
        pick: n(6)?,
        peel: n(7)?,
        cc: n(8)?,
        frontline: n(9)?,
        split: n(10)?,
        teamfight: n(11)?,
        early: n(12)?,
        mid: n(13)?,
        late: n(14)?,
        distance: c[15].trim() == "d",
    })
}

fn table() -> &'static HashMap<&'static str, Profil> {
    static TABLE: OnceLock<HashMap<&'static str, Profil>> = OnceLock::new();
    TABLE.get_or_init(|| {
        PROFILS
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(lire_ligne)
            .map(|p| (p.id, p))
            .collect()
    })
}

/// Profil d'un champion (id Data Dragon, ex. "MonkeyKing").
pub fn profil(id: &str) -> Option<&'static Profil> {
    table().get(id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Engage,
    Poke,
    Pick,
    Protect,
    Split,
    Teamfight,
}

impl Style {
    pub fn nom(self) -> &'static str {
        match self {
            Style::Engage => "Engage / all-in",
            Style::Poke => "Poke",
            Style::Pick => "Pick (attraper une cible)",
            Style::Protect => "Protect (protéger le carry)",
            Style::Split => "Split push",
            Style::Teamfight => "Teamfight",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnalyseEquipe {
    pub nombre: usize,
    /// Styles triés du plus fort au plus faible, avec leur score (0 à 3).
    pub styles: Vec<(Style, f32)>,
    pub ap_pct: u8,
    /// Sommes sur l'équipe (0 à 15).
    pub frontline: u8,
    pub cc: u8,
    /// Puissance moyenne (0 à 3) en early, mid, late.
    pub courbe: [f32; 3],
    pub alertes: Vec<String>,
    /// (id du champion, rôle dans la compo).
    pub roles: Vec<(&'static str, &'static str)>,
    // Moyennes utiles pour comparer / suggérer.
    moy_allin: f32,
    moy_pick: f32,
    moy_poke: f32,
    max_split: u8,
    max_engage: u8,
    max_frontline: u8,
}

impl AnalyseEquipe {
    pub fn style_principal(&self) -> Option<Style> {
        self.styles.first().map(|s| s.0)
    }

    /// Texte lisible. `nom` traduit un id Data Dragon en nom affiché.
    pub fn texte(&self, nom: &dyn Fn(&str) -> String) -> String {
        if self.nombre == 0 {
            return "  (pas encore de champions)".into();
        }
        let mut l = Vec::new();
        let principal = self.styles.first().map_or("?", |s| s.0.nom());
        let secondaires: Vec<&str> = self.styles.iter().skip(1).take(2).map(|s| s.0.nom()).collect();
        l.push(format!("  Style : {principal}  (aussi : {})", secondaires.join(", ")));
        l.push(format!(
            "  Dégâts {} % AP / {} % AD · frontline {}/15 · contrôle {}/15 · force early/mid/late {:.1} / {:.1} / {:.1}",
            self.ap_pct, 100 - self.ap_pct, self.frontline, self.cc, self.courbe[0], self.courbe[1], self.courbe[2]
        ));
        let roles: Vec<String> = self.roles.iter().map(|(id, r)| format!("{} = {r}", nom(id))).collect();
        l.push(format!("  Rôles : {}", roles.join(" · ")));
        for a in &self.alertes {
            l.push(format!("  ! {a}"));
        }
        l.join("\n")
    }
}

fn moyenne(profils: &[&Profil], f: impl Fn(&Profil) -> u8) -> f32 {
    if profils.is_empty() {
        return 0.0;
    }
    profils.iter().map(|p| f(p) as f32).sum::<f32>() / profils.len() as f32
}

/// Analyse une équipe (1 à 5 ids Data Dragon ; les inconnus sont ignorés).
pub fn analyser(equipe: &[&str]) -> AnalyseEquipe {
    let profils: Vec<&Profil> = equipe.iter().filter_map(|id| profil(id)).collect();
    let somme = |f: fn(&Profil) -> u8| profils.iter().map(|p| f(p)).sum::<u8>();
    let max = |f: fn(&Profil) -> u8| profils.iter().map(|p| f(p)).max().unwrap_or(0);

    let mut styles = vec![
        (Style::Engage, (moyenne(&profils, |p| p.engage) * 2.0 + moyenne(&profils, |p| p.allin)) / 3.0),
        (Style::Poke, moyenne(&profils, |p| p.poke)),
        (Style::Pick, moyenne(&profils, |p| p.pick)),
        (Style::Protect, moyenne(&profils, |p| p.peel)),
        (Style::Split, moyenne(&profils, |p| p.split)),
        (Style::Teamfight, moyenne(&profils, |p| p.teamfight)),
    ];
    styles.sort_by(|a, b| b.1.total_cmp(&a.1));

    let ap_pct = moyenne(&profils, |p| p.ap).round() as u8;
    let courbe = [moyenne(&profils, |p| p.early), moyenne(&profils, |p| p.mid), moyenne(&profils, |p| p.late)];

    // Rôle de chaque champion dans la compo (le premier critère qui colle).
    let max_engage = max(|p| p.engage);
    let mut engage_donne = false;
    let roles = profils
        .iter()
        .map(|p| {
            let r = if p.engage >= 2 && p.engage == max_engage && !engage_donne {
                engage_donne = true;
                "engage principal"
            } else if p.frontline <= 1 && p.peel <= 1 && (p.late >= 2 || p.mid >= 3) && p.poke <= 2 {
                if p.ap >= 50 { "carry AP" } else { "carry AD" }
            } else if p.peel >= 2 {
                "peel"
            } else if p.frontline >= 2 {
                "frontline"
            } else if p.poke >= 2 {
                "poke"
            } else if p.pick >= 2 {
                "pick"
            } else if p.split >= 2 {
                "split"
            } else if p.ap >= 50 {
                "dégâts AP"
            } else {
                "dégâts AD"
            };
            (p.id, r)
        })
        .collect();

    let mut alertes = Vec::new();
    let n = profils.len();
    if n >= 3 {
        if ap_pct < 25 {
            alertes.push("Presque full AD : l'ennemi n'a qu'à acheter de l'armure.".into());
        } else if ap_pct > 75 {
            alertes.push("Presque full AP : l'ennemi n'a qu'à acheter de la résistance magique.".into());
        }
        if max(|p| p.frontline) <= 1 {
            alertes.push("Aucune vraie frontline : personne pour encaisser devant.".into());
        }
        if max_engage <= 1 {
            alertes.push("Pas d'engage : il faudra jouer en réaction (contre-engage, poke).".into());
        }
        if somme(|p| p.cc) < n as u8 {
            alertes.push("Peu de contrôle : difficile d'attraper ou d'arrêter un ennemi.".into());
        }
        if courbe[0] >= courbe[2] + 0.6 {
            alertes.push("Compo très early : il faut prendre l'avance et finir avant 25-30 min.".into());
        } else if courbe[2] >= courbe[0] + 0.6 {
            alertes.push("Compo de scaling : jouez safe en early, vous êtes plus forts après 25 min.".into());
        }
        if profils.iter().all(|p| !p.distance) {
            alertes.push("Que des mêlées : une compo qui kite (poke, ralentissements) peut vous gêner.".into());
        }
    }

    AnalyseEquipe {
        nombre: n,
        styles,
        ap_pct,
        frontline: somme(|p| p.frontline),
        cc: somme(|p| p.cc),
        courbe,
        alertes,
        roles,
        moy_allin: moyenne(&profils, |p| p.allin),
        moy_pick: moyenne(&profils, |p| p.pick),
        moy_poke: moyenne(&profils, |p| p.poke),
        max_split: max(|p| p.split),
        max_engage,
        max_frontline: max(|p| p.frontline),
    }
}

/// Plan de jeu : 2 à 4 conseils selon les deux compos.
pub fn comparer(allies: &[&str], ennemis: &[&str]) -> Vec<String> {
    let a = analyser(allies);
    let e = analyser(ennemis);
    if a.nombre == 0 || e.nombre == 0 {
        return Vec::new();
    }
    let mut c = Vec::new();
    match (a.style_principal(), e.style_principal()) {
        (Some(Style::Poke), Some(Style::Engage | Style::Teamfight)) if e.max_engage >= 2 => {
            c.push("Vous pokez, ils engagent : gardez vos distances et pokez AVANT chaque objectif.".into())
        }
        (Some(Style::Engage | Style::Teamfight), Some(Style::Poke)) if a.max_engage >= 2 => {
            c.push("Ils pokent, vous engagez : ne restez pas sous leur poke, forcez le combat vite.".into())
        }
        (Some(Style::Engage), Some(Style::Protect)) => {
            c.push("Ils protègent leur carry : engagez sur le carry ou sur ceux qui sont mal placés.".into())
        }
        (Some(Style::Protect), Some(Style::Engage | Style::Pick)) => {
            c.push("Restez groupés autour de votre carry et gardez vos sorts de protection pour leur engage.".into())
        }
        (Some(Style::Split), _) => c.push("Compo de split : un joueur pousse sur le côté, les autres évitent le 5c5.".into()),
        _ => {}
    }
    let phases = ["early", "mid", "late"];
    let ecarts: Vec<f32> = (0..3).map(|i| a.courbe[i] - e.courbe[i]).collect();
    let (meilleure, pire) = (
        (0..3).max_by(|&x, &y| ecarts[x].total_cmp(&ecarts[y])).unwrap_or(0),
        (0..3).min_by(|&x, &y| ecarts[x].total_cmp(&ecarts[y])).unwrap_or(0),
    );
    if ecarts[meilleure] >= 0.4 {
        c.push(format!("Vous êtes plus forts en {} : c'est le moment de forcer les combats.", phases[meilleure]));
    }
    if ecarts[pire] <= -0.4 {
        c.push(format!("Ils sont plus forts en {} : évitez les combats à ce moment-là.", phases[pire]));
    }
    if e.moy_allin >= 1.8 {
        c.push("Ils ont beaucoup d'all-in : restez groupés et gardez Flash / sorts défensifs.".into());
    }
    if e.moy_pick >= 1.6 {
        c.push("Ils ont des champions de pick : pas de facecheck, wardez à plusieurs.".into());
    }
    if e.max_split >= 3 {
        c.push("Ils ont un split pusher : décidez qui répond sur le côté.".into());
    }
    if e.ap_pct < 25 && e.nombre >= 3 {
        c.push("Ils sont presque full AD : l'armure est très rentable.".into());
    } else if e.ap_pct > 75 && e.nombre >= 3 {
        c.push("Ils sont presque full AP : la résistance magique est très rentable.".into());
    }
    c.truncate(4);
    c
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    pub id: &'static str,
    pub score: f32,
    pub raison: String,
}

/// Champions jouables à `role` (format LCU) qui complètent la compo.
pub fn suggestions(allies: &[&str], ennemis: &[&str], role: &str, nombre: usize) -> Vec<Suggestion> {
    let a = analyser(allies);
    let e = analyser(ennemis);
    let pris = |id: &str| allies.contains(&id) || ennemis.contains(&id);
    let mut liste: Vec<Suggestion> = table()
        .values()
        .filter(|p| p.joue(role) && !pris(p.id))
        .map(|p| {
            // Chaque critère : (score, raison). On garde les 2 meilleures raisons.
            let mut criteres: Vec<(f32, &str)> = Vec::new();
            if a.nombre > 0 && a.ap_pct < 40 {
                criteres.push((p.ap as f32 / 100.0 * 2.0, "apporte des dégâts AP"));
            } else if a.nombre > 0 && a.ap_pct > 60 {
                criteres.push(((100 - p.ap) as f32 / 100.0 * 2.0, "apporte des dégâts AD"));
            }
            if a.max_frontline <= 1 {
                criteres.push((p.frontline as f32 * 0.7, "donne une frontline"));
            }
            if a.max_engage <= 1 {
                criteres.push((p.engage as f32 * 0.7, "donne de l'engage"));
            }
            if e.moy_allin >= 1.5 {
                criteres.push((p.peel as f32 * 0.6, "protège contre leur all-in"));
            }
            if e.moy_poke >= 1.5 {
                criteres.push((p.engage as f32 * 0.4, "peut engager sur leur poke"));
            }
            criteres.push((p.teamfight as f32 * 0.3, "bon en teamfight"));
            criteres.sort_by(|x, y| y.0.total_cmp(&x.0));
            let score = criteres.iter().map(|c| c.0).sum();
            let raison = criteres.iter().filter(|c| c.0 > 0.0).take(2).map(|c| c.1).collect::<Vec<_>>().join(", ");
            Suggestion { id: p.id, score, raison }
        })
        .collect();
    liste.sort_by(|x, y| y.score.total_cmp(&x.score).then(x.id.cmp(y.id)));
    liste.truncate(nombre);
    liste
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_fichier_est_propre() {
        let mut vus = std::collections::HashSet::new();
        let lignes: Vec<&str> = PROFILS.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).collect();
        for l in &lignes {
            let p = lire_ligne(l).unwrap_or_else(|| panic!("ligne invalide : {l}"));
            assert!(vus.insert(p.id), "doublon : {}", p.id);
            assert!(p.ap <= 100);
            for note in [p.engage, p.poke, p.allin, p.pick, p.peel, p.cc, p.frontline, p.split, p.teamfight, p.early, p.mid, p.late] {
                assert!(note <= 3, "note hors bornes pour {}", p.id);
            }
            assert!(p.roles.split(',').all(|r| crate::stats::ROLES.contains(&r)), "rôle inconnu pour {}", p.id);
        }
        assert!(lignes.len() >= 165);
    }

    #[test]
    fn reconnait_les_styles() {
        let engage = analyser(&["Malphite", "Amumu", "Orianna", "MissFortune", "Leona"]);
        assert!(matches!(engage.style_principal(), Some(Style::Engage | Style::Teamfight)), "{:?}", engage.styles);
        let poke = analyser(&["Jayce", "Nidalee", "Xerath", "Ezreal", "Lux"]);
        assert_eq!(poke.style_principal(), Some(Style::Poke), "{:?}", poke.styles);
    }

    #[test]
    fn alerte_full_ad() {
        let a = analyser(&["Zed", "Draven", "Talon", "Kindred", "Pyke"]);
        assert!(a.alertes.iter().any(|x| x.contains("full AD")));
    }

    #[test]
    fn suggestions_valides() {
        let allies = ["Zed", "Draven", "Talon", "Kindred"];
        let s = suggestions(&allies, &["Ahri"], "top", 5);
        assert_eq!(s.len(), 5);
        for x in &s {
            assert!(profil(x.id).unwrap().joue("top"));
            assert!(!allies.contains(&x.id) && x.id != "Ahri");
        }
    }
}
