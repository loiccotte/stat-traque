//! Formulaire de l'onglet « Réglages » : traduit `reglages.txt` (texte `nom=valeur`)
//! en cases à cocher, et inversement. Fonctions pures, testées sans écran.

use crate::stockage::Reglages;

/// Serveurs proposés dans la liste déroulante (code Riot, libellé).
pub const REGIONS: [(&str, &str); 16] = [
    ("euw1", "Europe Ouest (EUW)"),
    ("eun1", "Europe Nord & Est (EUNE)"),
    ("tr1", "Turquie (TR)"),
    ("ru", "Russie (RU)"),
    ("me1", "Moyen-Orient (ME)"),
    ("na1", "Amérique du Nord (NA)"),
    ("br1", "Brésil (BR)"),
    ("la1", "Amérique latine Nord (LAN)"),
    ("la2", "Amérique latine Sud (LAS)"),
    ("kr", "Corée (KR)"),
    ("jp1", "Japon (JP)"),
    ("oc1", "Océanie (OCE)"),
    ("ph2", "Philippines (PH)"),
    ("sg2", "Singapour (SG)"),
    ("tw2", "Taïwan (TW)"),
    ("vn2", "Viêt Nam (VN)"),
];

/// Les réglages modifiables depuis la fenêtre, sous forme de cases à cocher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormReglages {
    /// Flash sur F (sinon sur D).
    pub flash_sur_f: bool,
    pub runes_auto: bool,
    pub sorts_auto: bool,
    pub enregistrer: bool,
    /// Ouvrir la fenêtre toute seule au début du champ select.
    pub fenetre_auto: bool,
    pub region: String,
}

/// « non » désactive ; tout le reste (absent compris) = activé, comme dans le reste de l'app.
fn active(r: &Reglages, nom: &str) -> bool {
    r.get(nom) != Some("non")
}

fn oui_non(b: bool) -> &'static str {
    if b { "oui" } else { "non" }
}

impl FormReglages {
    pub fn depuis(r: &Reglages) -> Self {
        Self {
            flash_sur_f: r.get("flash").is_some_and(|f| f.eq_ignore_ascii_case("f")),
            runes_auto: active(r, "runes"),
            sorts_auto: active(r, "sorts"),
            enregistrer: active(r, "enregistrer"),
            fenetre_auto: active(r, "fenetre_auto"),
            region: r.region().to_string(),
        }
    }

    /// Recopie le formulaire dans les réglages (les autres lignes du fichier sont gardées).
    pub fn appliquer(&self, r: &mut Reglages) {
        r.set("flash", if self.flash_sur_f { "f" } else { "d" });
        r.set("runes", oui_non(self.runes_auto));
        r.set("sorts", oui_non(self.sorts_auto));
        r.set("enregistrer", oui_non(self.enregistrer));
        r.set("fenetre_auto", oui_non(self.fenetre_auto));
        r.set("region", &self.region);
    }
}

/// Libellé d'une région (ou le code brut si elle n'est pas dans la liste).
pub fn libelle_region(code: &str) -> &str {
    REGIONS.iter().find(|(c, _)| *c == code).map_or(code, |(_, l)| l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valeurs_par_defaut() {
        let f = FormReglages::depuis(&Reglages::default());
        assert!(!f.flash_sur_f && f.runes_auto && f.sorts_auto && f.enregistrer && f.fenetre_auto);
        assert_eq!(f.region, "euw1");
    }

    #[test]
    fn aller_retour() {
        let mut r = Reglages::depuis_texte("flash=F\nruns=x\nsorts=non\ndossier_lol=C:\\LoL\n");
        let mut f = FormReglages::depuis(&r);
        assert!(f.flash_sur_f && !f.sorts_auto);
        f.fenetre_auto = false;
        f.region = "kr".into();
        f.appliquer(&mut r);
        assert_eq!(r.get("fenetre_auto"), Some("non"));
        assert_eq!(r.get("flash"), Some("f"));
        assert_eq!(r.region(), "kr");
        assert_eq!(r.get("dossier_lol"), Some("C:\\LoL"), "les autres réglages sont gardés");
        assert_eq!(FormReglages::depuis(&r), f);
    }

    #[test]
    fn libelles() {
        assert_eq!(libelle_region("euw1"), "Europe Ouest (EUW)");
        assert_eq!(libelle_region("xx9"), "xx9");
    }
}
