//! Mise en forme des textes de l'interface : fonctions PURES (elles prennent des
//! données et rendent du texte, sans rien dessiner). On peut donc les tester avec
//! `cargo test`, même sans écran.

use crate::ddragon::DonneesJeu;
use crate::gameflow::Phase;
use crate::rangs::{InfoJoueur, Rang};

/// Nombre à virgule « à la française » : `52.14` -> `"52,1"`.
pub fn decimal(x: f32, chiffres: usize) -> String {
    format!("{x:.chiffres$}").replace('.', ",")
}

/// Pourcentage : `52.14` -> `"52,1 %"` (espace insécable avant le %, comme en français).
pub fn pourcent(x: f32) -> String {
    format!("{}\u{a0}%", decimal(x, 1))
}

/// Nombre avec `d` décimales ; sans décimale, groupé par milliers (`23400` -> `"23 400"`).
pub fn nombre(x: f32, d: usize) -> String {
    if d == 0 { milliers(x.max(0.0).round() as u32) } else { decimal(x, d) }
}

/// Grands nombres lisibles : `3400` -> `"3 400"`.
pub fn milliers(n: u32) -> String {
    let chiffres = n.to_string();
    let mut sortie = String::new();
    for (i, c) in chiffres.chars().enumerate() {
        // Une espace avant chaque groupe de 3 chiffres (en partant de la droite).
        if i > 0 && (chiffres.len() - i).is_multiple_of(3) {
            sortie.push('\u{a0}');
        }
        sortie.push(c);
    }
    sortie
}

/// Durée de partie : `725` secondes -> `"12:05"`.
pub fn duree(secondes: u32) -> String {
    format!("{}:{:02}", secondes / 60, secondes % 60)
}

/// `"5/2/7"`.
pub fn kda(kills: u16, morts: u16, assists: u16) -> String {
    format!("{kills}/{morts}/{assists}")
}

/// Ratio KDA : (kills + assists) / morts ; « parfait » sans mort.
pub fn ratio_kda(kills: u16, morts: u16, assists: u16) -> String {
    if morts == 0 {
        return "parfait".into();
    }
    decimal((kills + assists) as f32 / morts as f32, 1)
}

/// Rang solo, ou « Non classé ».
pub fn rang_ou_non_classe(rang: Option<&Rang>) -> String {
    rang.map_or_else(|| "Non classé".into(), |r| r.to_string())
}

/// `"54 % · 120 parties"` (vide si aucune partie).
pub fn bilan(rang: &Rang) -> String {
    let parties = rang.victoires as u32 + rang.defaites as u32;
    if parties == 0 {
        return String::new();
    }
    format!("{} · {} parties", pourcent_entier(rang.winrate()), milliers(parties))
}

/// Pourcentage sans décimale : `54.4` -> `"54 %"`.
pub fn pourcent_entier(x: f32) -> String {
    format!("{x:.0}\u{a0}%")
}

/// Maîtrise : `(7, 245_300)` -> `"M7 · 245 k"`.
pub fn maitrise(m: Option<(u8, u32)>) -> String {
    m.map_or_else(String::new, |(niveau, points)| format!("M{niveau} · {} k", milliers(points / 1000)))
}

/// Meilleur rang déjà vu, seulement s'il est AU-DESSUS du rang actuel
/// (sinon l'info n'apprend rien).
pub fn meilleur_vu(j: &InfoJoueur) -> Option<String> {
    match (&j.meilleur, &j.solo) {
        (Some(m), Some(s)) if m.valeur() > s.valeur() => Some(m.to_string()),
        (Some(m), None) => Some(m.to_string()),
        _ => None,
    }
}

/// Rang très court pour l'overlay : `"E2 45"`, `"M 120"`, `"—"`.
pub fn rang_court(rang: Option<&Rang>) -> String {
    let Some(r) = rang else { return "—".into() };
    let lettre = match &*r.palier {
        "GRANDMASTER" => "GM",
        "CHALLENGER" => "Chall",
        p => &p[..1],
    };
    let division = match &*r.division {
        "I" => "1",
        "II" => "2",
        "III" => "3",
        "IV" => "4",
        _ => "",
    };
    if division.is_empty() {
        format!("{lettre} {} LP", r.lp)
    } else {
        format!("{lettre}{division} {} LP", r.lp)
    }
}

/// Couleur (rouge, vert, bleu) associée à un palier, proche de celle du client.
pub fn couleur_palier(palier: &str) -> [u8; 3] {
    match palier {
        "IRON" => [120, 110, 105],
        "BRONZE" => [176, 120, 82],
        "SILVER" => [160, 175, 185],
        "GOLD" => [220, 180, 90],
        "PLATINUM" => [70, 190, 170],
        "EMERALD" => [40, 190, 110],
        "DIAMOND" => [110, 150, 255],
        "MASTER" => [190, 100, 230],
        "GRANDMASTER" => [230, 80, 80],
        "CHALLENGER" => [240, 210, 120],
        _ => [150, 150, 150],
    }
}

/// Nom affiché d'un champion à partir de son id numérique.
pub fn nom_champion(donnees: Option<&DonneesJeu>, id: u16) -> String {
    match donnees {
        Some(d) => d.nom_champion(id).to_string(),
        None => format!("#{id}"),
    }
}

/// Nom affiché d'un champion à partir de son id TEXTE Data Dragon ("MonkeyKing" -> "Wukong").
pub fn nom_depuis_id_dd(donnees: Option<&DonneesJeu>, id: &str) -> String {
    donnees
        .and_then(|d| d.champions.values().find(|c| &*c.id == id))
        .map_or_else(|| id.to_string(), |c| c.nom.to_string())
}

/// Noms des objets d'un joueur (les inconnus sont ignorés).
pub fn noms_objets(donnees: Option<&DonneesJeu>, items: &[u32]) -> Vec<String> {
    let Some(d) = donnees else { return Vec::new() };
    items.iter().filter_map(|i| d.objets.get(i)).map(|n| n.to_string()).collect()
}

/// Moment fort d'une compo d'après sa courbe early / mid / late (0 à 3).
pub fn moment_fort(courbe: [f32; 3]) -> &'static str {
    let [early, mid, late] = courbe;
    if (early - late).abs() < 0.3 && (mid - early).abs() < 0.3 {
        "équilibrée"
    } else if early >= mid && early >= late {
        "forte en early"
    } else if late >= mid {
        "forte en late"
    } else {
        "forte en mid game"
    }
}

/// Mon rôle LCU ("utility") traduit en libellé lisible.
pub fn libelle_role(role: &str) -> &str {
    match role {
        "top" => "Top",
        "jungle" => "Jungle",
        "middle" | "mid" => "Mid",
        "bottom" | "adc" => "ADC",
        "utility" | "support" => "Support",
        autre => autre,
    }
}

/// Fraction (0 à 1) d'une note sur un maximum, bornée.
pub fn fraction(valeur: f32, max: f32) -> f32 {
    if max <= 0.0 {
        return 0.0;
    }
    (valeur / max).clamp(0.0, 1.0)
}

/// Les deux équipes, bleue d'abord, chacune dans l'ordre reçu.
pub fn par_equipe(joueurs: &[InfoJoueur]) -> (Vec<&InfoJoueur>, Vec<&InfoJoueur>) {
    joueurs.iter().partition(|j| j.bleu)
}

/// Texte de statut affiché en haut de la fenêtre.
pub fn statut(connecte: bool, phase: Option<Phase>, cle_api: bool) -> String {
    let client = if connecte { "Client LoL connecté" } else { "En attente du client LoL" };
    let phase = phase.map_or("—", Phase::libelle);
    let cle = if cle_api { "clé API OK" } else { "pas de clé API (rangs indisponibles)" };
    format!("{client} · {phase} · {cle}")
}

/// Position de l'overlay lue dans les réglages (`overlay_position=x,y`).
pub fn position_overlay(texte: Option<&str>) -> [f32; 2] {
    const DEFAUT: [f32; 2] = [12.0, 12.0];
    let Some((x, y)) = texte.and_then(|t| t.split_once(',')) else { return DEFAUT };
    match (x.trim().parse(), y.trim().parse()) {
        (Ok(x), Ok(y)) => [x, y],
        _ => DEFAUT,
    }
}

/// Date relative : « il y a 5 min », « il y a 3 h », « il y a 2 j ».
pub fn il_y_a(maintenant: u64, date: u64) -> String {
    let s = maintenant.saturating_sub(date);
    match s {
        0..=59 => "à l'instant".into(),
        60..=3599 => format!("il y a {} min", s / 60),
        3600..=86_399 => format!("il y a {} h", s / 3600),
        _ => format!("il y a {} j", s / 86_400),
    }
}

/// Secondes Unix actuelles.
pub fn maintenant() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Variation de LP avec son signe : `+21 LP`, `-18 LP`.
pub fn variation_lp(lp: i32) -> String {
    format!("{lp:+} LP")
}

/// Écart à la moyenne : `+13 %` (None si pas de moyenne).
pub fn ecart(ecart_pct: Option<f32>) -> String {
    ecart_pct.map_or_else(String::new, |e| format!("{e:+.0}\u{a0}%"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_et_ecarts() {
        assert_eq!(il_y_a(1000, 990), "à l'instant");
        assert_eq!(il_y_a(10_000, 10_000 - 300), "il y a 5 min");
        assert_eq!(il_y_a(100_000, 100_000 - 3 * 3600), "il y a 3 h");
        assert_eq!(il_y_a(1_000_000, 1_000_000 - 2 * 86_400), "il y a 2 j");
        assert_eq!(il_y_a(0, 10), "à l'instant");
        assert_eq!(variation_lp(21), "+21 LP");
        assert_eq!(variation_lp(-18), "-18 LP");
        assert_eq!(ecart(Some(13.2)), "+13\u{a0}%");
        assert_eq!(ecart(None), "");
    }

    use super::*;

    fn rang(palier: &str, division: &str, lp: u16, v: u16, d: u16) -> Rang {
        Rang { palier: palier.into(), division: division.into(), lp, victoires: v, defaites: d, serie: false }
    }

    #[test]
    fn nombres_a_la_francaise() {
        assert_eq!(decimal(52.14, 1), "52,1");
        assert_eq!(pourcent(52.14), "52,1\u{a0}%");
        assert_eq!(milliers(3400), "3\u{a0}400");
        assert_eq!(milliers(999), "999");
        assert_eq!(milliers(1_234_567), "1\u{a0}234\u{a0}567");
        assert_eq!(milliers(0), "0");
        assert_eq!(nombre(23_400.4, 0), "23\u{a0}400");
        assert_eq!(nombre(3.94, 1), "3,9");
    }

    #[test]
    fn duree_et_kda() {
        assert_eq!(duree(725), "12:05");
        assert_eq!(duree(59), "0:59");
        assert_eq!(kda(5, 2, 7), "5/2/7");
        assert_eq!(ratio_kda(5, 2, 7), "6,0");
        assert_eq!(ratio_kda(3, 0, 1), "parfait");
    }

    #[test]
    fn rangs() {
        let r = rang("EMERALD", "II", 45, 65, 55);
        assert_eq!(rang_ou_non_classe(Some(&r)), "Emerald II 45 LP");
        assert_eq!(rang_ou_non_classe(None), "Non classé");
        assert_eq!(bilan(&r), "54\u{a0}% · 120 parties");
        assert_eq!(bilan(&rang("GOLD", "I", 0, 0, 0)), "");
        assert_eq!(rang_court(Some(&r)), "E2 45 LP");
        assert_eq!(rang_court(Some(&rang("MASTER", "", 120, 1, 1))), "M 120 LP");
        assert_eq!(rang_court(Some(&rang("GRANDMASTER", "", 500, 1, 1))), "GM 500 LP");
        assert_eq!(rang_court(None), "—");
        assert_eq!(maitrise(Some((7, 245_300))), "M7 · 245 k");
        assert_eq!(maitrise(None), "");
    }

    #[test]
    fn meilleur_vu_seulement_si_superieur() {
        let mut j = InfoJoueur {
            nom: "a".into(),
            champion: 1,
            bleu: true,
            moi: false,
            solo: Some(rang("GOLD", "II", 10, 1, 1)),
            flex: None,
            meilleur: Some(rang("GOLD", "II", 10, 1, 1)),
            maitrise: None,
            badges: Vec::new(),
        };
        assert_eq!(meilleur_vu(&j), None);
        j.meilleur = Some(rang("PLATINUM", "IV", 0, 1, 1));
        assert_eq!(meilleur_vu(&j).as_deref(), Some("Platinum IV 0 LP"));
        j.solo = None;
        assert!(meilleur_vu(&j).is_some());
    }

    #[test]
    fn moment_fort_de_la_compo() {
        assert_eq!(moment_fort([2.5, 1.5, 1.0]), "forte en early");
        assert_eq!(moment_fort([1.0, 1.5, 2.5]), "forte en late");
        assert_eq!(moment_fort([1.0, 2.5, 1.2]), "forte en mid game");
        assert_eq!(moment_fort([2.0, 2.1, 2.0]), "équilibrée");
    }

    #[test]
    fn divers() {
        assert_eq!(libelle_role("utility"), "Support");
        assert_eq!(libelle_role("bottom"), "ADC");
        assert_eq!(fraction(7.5, 15.0), 0.5);
        assert_eq!(fraction(20.0, 15.0), 1.0);
        assert_eq!(fraction(1.0, 0.0), 0.0);
        assert_eq!(position_overlay(Some(" 100 , 40 ")), [100.0, 40.0]);
        assert_eq!(position_overlay(Some("n'importe quoi")), [12.0, 12.0]);
        assert_eq!(position_overlay(None), [12.0, 12.0]);
        assert_eq!(couleur_palier("GOLD"), [220, 180, 90]);
        assert!(statut(true, Some(Phase::ChampSelect), false).contains("Champ select"));
        assert_eq!(nom_champion(None, 238), "#238");
        assert_eq!(nom_depuis_id_dd(None, "MonkeyKing"), "MonkeyKing");
        assert!(noms_objets(None, &[1001]).is_empty());
    }
}
