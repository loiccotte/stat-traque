//! Data Dragon : les données publiques de Riot (noms des champions, runes, sorts, objets).
//!
//! Optimisation :
//!  - téléchargé UNE fois par patch, puis relu depuis le disque (démarrage instantané,
//!    fonctionne hors ligne) ;
//!  - sur le disque on ne garde qu'un petit fichier texte `id<TAB>nom` par type,
//!    pas les gros JSON (~1 Mo) de Riot ;
//!  - chargé dans un thread à part : l'app est utilisable avant que ce soit fini.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use serde_json::Value;

use crate::evenement::Evenement;
use crate::log;

const CDN: &str = "https://ddragon.leagueoflegends.com";
const LANGUE: &str = "fr_FR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Champion {
    /// Identifiant texte, sert pour les images (ex : "MonkeyKing" pour Wukong).
    pub id: Box<str>,
    pub nom: Box<str>,
}

/// Toutes les données du patch, en mémoire (quelques dizaines de Ko).
#[derive(Debug, Default)]
pub struct DonneesJeu {
    pub version: String,
    pub champions: HashMap<u16, Champion>,
    pub sorts: HashMap<u16, Box<str>>,
    pub runes: HashMap<u16, Box<str>>,
    /// u32 : certains objets ont un identifiant à 6 chiffres.
    pub objets: HashMap<u32, Box<str>>,
    /// Genre des objets qui comptent pour les builds : SEULEMENT les objets
    /// légendaires et les bottes de la Faille (~120 entrées, ~2 Ko en RAM).
    /// Vide si le cache est à l'ancien format (re-téléchargé dès qu'internet est là).
    pub genres_objets: HashMap<u32, GenreObjet>,
}

/// Les objets qu'on compte dans les builds (stats.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GenreObjet {
    /// Objet « complet » : ne s'améliore en rien, cher, achetable sur la Faille.
    Legendaire,
    /// Bottes améliorées (pas les Bottes de base à 300 PO).
    Bottes,
}

impl DonneesJeu {
    pub fn nom_champion(&self, id: u16) -> &str {
        self.champions.get(&id).map_or("inconnu", |c| &c.nom)
    }

    /// Légendaire, bottes, ou `None` pour tout le reste (composants, potions...).
    pub fn genre_objet(&self, id: u32) -> Option<GenreObjet> {
        self.genres_objets.get(&id).copied()
    }

    /// Nom affiché d'un objet (l'id en texte s'il est inconnu).
    pub fn nom_objet(&self, id: u32) -> String {
        self.objets.get(&id).map_or_else(|| id.to_string(), |n| n.to_string())
    }
}

/// Lance le chargement en arrière-plan ; envoie `DonneesPretes` quand c'est fait.
pub fn charger_en_fond(tx: Sender<Evenement>) {
    std::thread::Builder::new()
        .name("data-dragon".into())
        .spawn(move || {
            let debut = std::time::Instant::now();
            match charger() {
                Some(d) => {
                    log!(
                        "Data Dragon {} : {} champions, {} runes, {} objets dont {} légendaires/bottes ({} ms)",
                        d.version,
                        d.champions.len(),
                        d.runes.len(),
                        d.objets.len(),
                        d.genres_objets.len(),
                        debut.elapsed().as_millis()
                    );
                    let _ = tx.send(Evenement::DonneesPretes(Arc::new(d)));
                }
                None => log!("Data Dragon indisponible (pas d'internet et pas de cache)."),
            }
        })
        .expect("impossible de lancer le thread Data Dragon");
}

/// Version la plus récente selon Riot (`None` si pas d'internet).
fn version_en_ligne() -> Option<String> {
    crate::http::internet()
        .get(&format!("{CDN}/api/versions.json"))
        .call()
        .ok()?
        .into_json::<Value>()
        .ok()?[0]
        .as_str()
        .map(String::from)
}

/// Un patch est complet si `objets.tsv` (écrit en dernier) existe.
fn complet(racine: &Path, version: &str) -> bool {
    racine.join(version).join("objets.tsv").exists()
}

/// Première ligne de `objets.tsv` au format actuel. Les anciens caches (format 1 :
/// `id<TAB>nom` seulement) n'ont pas cette ligne -> on les re-télécharge.
/// (Format 3 : ajoute les drapeaux D et R ; un cache au format 2 est re-téléchargé.)
const FORMAT_OBJETS: &str = "#format\t3";

/// Complet ET au format actuel. On ne lit que les premiers octets du fichier.
fn a_jour(racine: &Path, version: &str) -> bool {
    use std::io::Read;
    let mut debut = [0u8; FORMAT_OBJETS.len()];
    std::fs::File::open(racine.join(version).join("objets.tsv"))
        .and_then(|mut f| f.read_exact(&mut debut))
        .is_ok_and(|_| debut == FORMAT_OBJETS.as_bytes())
}

/// Choisit le patch à utiliser :
///  1. le dernier patch en ligne (téléchargé si besoin) ;
///  2. sinon (hors ligne, ou échec du téléchargement) le dernier patch complet sur le disque.
fn choisir_version(racine: &Path) -> Option<String> {
    let fichier = racine.join("version.txt");
    if let Some(v) = version_en_ligne() {
        // Patch déjà là mais à l'ancien format : on le re-télécharge comme un nouveau
        // patch (en cas d'échec, le repli plus bas garde l'ancien, encore lisible).
        if a_jour(racine, &v) {
            return Some(v);
        }
        log!("Patch {v} : téléchargement des données...");
        if telecharger(&v, &racine.join(&v)).is_some() {
            // version.txt n'est écrit QU'APRÈS un téléchargement réussi.
            let _ = std::fs::write(&fichier, &v);
            nettoyer_anciens_patchs(racine, &v);
            return Some(v);
        }
        log!("Téléchargement du patch {v} raté : on garde le patch précédent.");
    }
    let v = std::fs::read_to_string(fichier).ok()?.trim().to_string();
    complet(racine, &v).then_some(v)
}

pub fn charger() -> Option<DonneesJeu> {
    let racine = crate::stockage::dossier_cache("ddragon");
    let version = choisir_version(&racine)?;
    Some(lire_patch(&racine, version))
}

/// Le patch déjà en cache, SANS rien demander à internet (processus de la fenêtre :
/// le processus principal s'est déjà occupé des téléchargements).
pub fn charger_cache() -> Option<DonneesJeu> {
    let racine = crate::stockage::dossier_cache("ddragon");
    let v = std::fs::read_to_string(racine.join("version.txt")).ok()?.trim().to_string();
    complet(&racine, &v).then(|| lire_patch(&racine, v))
}

fn lire_patch(racine: &Path, version: String) -> DonneesJeu {
    let dossier = racine.join(&version);
    let lire = |nom: &str| std::fs::read_to_string(dossier.join(nom)).unwrap_or_default();
    let (objets, genres_objets) = lire_objets(&lire("objets.tsv"));
    DonneesJeu {
        champions: lire_tsv(&lire("champions.tsv"))
            .map(|(cle, reste)| {
                let (id, nom) = reste.split_once('\t').unwrap_or((reste, reste));
                (cle, Champion { id: id.into(), nom: nom.into() })
            })
            .collect(),
        sorts: lire_tsv(&lire("sorts.tsv")).map(|(k, v)| (k, v.into())).collect(),
        runes: lire_tsv(&lire("runes.tsv")).map(|(k, v)| (k, v.into())).collect(),
        objets,
        genres_objets,
        version,
    }
}

/// Lit objets.tsv UNE fois et en tire les deux tables (noms, genres).
fn lire_objets(texte: &str) -> (HashMap<u32, Box<str>>, HashMap<u32, GenreObjet>) {
    let mut noms = HashMap::new();
    let mut genres = HashMap::new();
    for (id, reste) in lire_tsv::<u32>(texte) {
        noms.insert(id, nom_objet_tsv(reste).into());
        if let Some(g) = genre_objet_tsv(reste) {
            genres.insert(id, g);
        }
    }
    (noms, genres)
}

// ---------- objets.tsv (format 2) ----------
// Une ligne par objet : id<TAB>nom<TAB>prix total<TAB>profondeur<TAB>drapeaux
// Drapeaux (lettres, "-" si aucune) :
//   F = final : son `into` ne contient AUCUN objet achetable sur la Faille
//       (Manamune -> Muramana : Muramana ne s'achète pas, donc Manamune est final)
//   A = achetable sur la Faille (carte 11, en boutique, pas réservé à un champion)
//   D = dérivé : se construit à partir d'autres objets (`from` non vide)
//   B = bottes (tag "Boots"), sauf les Bottes de base (elles n'ont pas de `from`)
//   C = consommable (potions, élixirs...)
//   R = transformation (`specialRecipe`) sur la Faille : Muramana, Séraphin,
//       Fimbulwinter... Pas achetable, mais c'est bien un objet complet.
// On garde les infos brutes (prix, profondeur) : la règle « légendaire » peut
// évoluer sans re-télécharger.

/// Prix minimum d'un objet complet. Assez bas pour garder Mejai (1 500 PO),
/// assez haut pour écarter les petits objets finaux bon marché.
const PRIX_MIN_LEGENDAIRE: u32 = 1000;

/// Colonne « nom » d'une ligne sans l'id (ancien et nouveau format).
fn nom_objet_tsv(reste: &str) -> &str {
    reste.split('\t').next().unwrap_or(reste)
}

/// Genre d'un objet d'après sa ligne sans l'id. `None` pour l'ancien format.
fn genre_objet_tsv(reste: &str) -> Option<GenreObjet> {
    let c: Vec<&str> = reste.split('\t').collect();
    let prix: u32 = c.get(1)?.parse().ok()?;
    let drapeaux = *c.get(3)?;
    let a = |lettre: char| drapeaux.contains(lettre);
    if a('C') || !(a('A') || a('R')) {
        return None;
    }
    // Bottes d'abord : des bottes « transformées » restent des bottes.
    if a('B') {
        return Some(GenreObjet::Bottes);
    }
    // Transformation (Muramana...) : objet complet même s'il ne s'achète pas.
    if a('R') {
        return Some(GenreObjet::Legendaire);
    }
    // Final, construit à partir d'autres objets, et pas trop bon marché.
    let legendaire = a('F') && a('D') && prix >= PRIX_MIN_LEGENDAIRE;
    legendaire.then_some(GenreObjet::Legendaire)
}

/// Lit des lignes `id<TAB>reste`.
fn lire_tsv<K: std::str::FromStr>(texte: &str) -> impl Iterator<Item = (K, &str)> {
    texte.lines().filter_map(|l| {
        let (id, reste) = l.split_once('\t')?;
        Some((id.parse().ok()?, reste))
    })
}

fn json(url: &str) -> Option<Value> {
    crate::http::internet().get(url).call().ok()?.into_json().ok()
}

/// Télécharge les 4 fichiers du patch et les convertit en petits TSV.
/// UN fichier à la fois : téléchargé, converti, écrit puis libéré
/// (item.json pèse plusieurs Mo une fois décodé : on n'en garde jamais deux en RAM).
fn telecharger(version: &str, dossier: &Path) -> Option<()> {
    let base = format!("{CDN}/cdn/{version}/data/{LANGUE}");
    std::fs::create_dir_all(dossier).ok()?;
    // (fichier Riot, fichier TSV, fonction de conversion)
    type Etape = (&'static str, &'static str, fn(&Value) -> Vec<String>);
    let etapes: [Etape; 4] = [
        ("champion.json", "champions.tsv", tsv_champions),
        ("summoner.json", "sorts.tsv", tsv_par_cle),
        ("runesReforged.json", "runes.tsv", tsv_runes),
        // objets.tsv en dernier : sa présence signifie "patch complet".
        ("item.json", "objets.tsv", tsv_objets),
    ];
    for (source, cible, convertir) in etapes {
        let lignes = convertir(&json(&format!("{base}/{source}"))?);
        std::fs::write(dossier.join(cible), lignes.join("\n")).ok()?;
    }
    Some(())
}

/// champion.json : {"data": {"Ahri": {"key": "103", "id": "Ahri", "name": "Ahri"}}}
fn tsv_champions(v: &Value) -> Vec<String> {
    v["data"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, c)| Some(format!("{}\t{}\t{}", c["key"].as_str()?, c["id"].as_str()?, c["name"].as_str()?)))
        .collect()
}

/// summoner.json : même forme, "key" numérique en texte.
fn tsv_par_cle(v: &Value) -> Vec<String> {
    v["data"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, s)| Some(format!("{}\t{}", s["key"].as_str()?, s["name"].as_str()?)))
        .collect()
}

/// item.json : {"data": {"3020": {"name": "...", "gold": {"total": 1100, "purchasable": true},
///   "tags": ["Boots"], "from": [...], "into": [...], "depth": 2, "maps": {"11": true}}}}
/// (l'id est la clé). Écrit le format 2 décrit plus haut.
fn tsv_objets(v: &Value) -> Vec<String> {
    let mut lignes = vec![FORMAT_OBJETS.to_string()];
    let Some(data) = v["data"].as_object() else { return lignes };
    // Objet sur la Faille et pas réservé (à un champion, ou à l'allié Ornn).
    let sur_la_faille = |o: &Value| {
        let reserve = |cle: &str| o[cle].as_str().is_some_and(|s| !s.is_empty());
        o["maps"]["11"] == true && !reserve("requiredChampion") && !reserve("requiredAlly")
    };
    let achetable = |o: &Value| sur_la_faille(o) && o["gold"]["purchasable"] != false && o["inStore"] != false;
    // 1er passage : les ids achetables sur la Faille (pour le drapeau F).
    let achetables: std::collections::HashSet<&str> =
        data.iter().filter(|(_, o)| achetable(o)).map(|(id, _)| id.as_str()).collect();
    lignes.extend(data.iter().filter_map(|(id, o)| {
        // Champ absent ou liste vide : même chose pour nous.
        let vide = |cle: &str| o[cle].as_array().is_none_or(|t| t.is_empty());
        let a_tag = |t: &str| o["tags"].as_array().is_some_and(|ts| ts.iter().any(|x| x == t));
        // Final : rien d'achetable au-dessus de lui.
        let final_ = o["into"]
            .as_array()
            .is_none_or(|t| !t.iter().any(|x| x.as_str().is_some_and(|i| achetables.contains(i))));
        let transformation = sur_la_faille(o) && o["specialRecipe"].as_u64().is_some_and(|r| r != 0);
        let mut drapeaux = String::new();
        for (oui, lettre) in [
            (final_, 'F'),
            (achetable(o), 'A'),
            (!vide("from"), 'D'),
            (a_tag("Boots") && !vide("from"), 'B'),
            (a_tag("Consumable") || o["consumed"] == true, 'C'),
            (transformation, 'R'),
        ] {
            if oui {
                drapeaux.push(lettre);
            }
        }
        if drapeaux.is_empty() {
            drapeaux.push('-');
        }
        Some(format!(
            "{id}\t{}\t{}\t{}\t{drapeaux}",
            o["name"].as_str()?,
            o["gold"]["total"].as_u64().unwrap_or(0),
            o["depth"].as_u64().unwrap_or(1)
        ))
    }));
    lignes
}

/// runesReforged.json : [{id, name, slots: [{runes: [{id, name}]}]}]
/// On garde les arbres (Précision, Domination...) ET les runes.
fn tsv_runes(v: &Value) -> Vec<String> {
    let mut lignes = Vec::new();
    for arbre in v.as_array().into_iter().flatten() {
        lignes.push(format!("{}\t{}", arbre["id"], arbre["name"].as_str().unwrap_or("?")));
        for rune in arbre["slots"].as_array().into_iter().flatten().flat_map(|s| s["runes"].as_array().into_iter().flatten()) {
            lignes.push(format!("{}\t{}", rune["id"], rune["name"].as_str().unwrap_or("?")));
        }
    }
    lignes
}

/// Supprime les dossiers des anciens patchs (le disque ne grossit pas avec le temps).
fn nettoyer_anciens_patchs(racine: &Path, garder: &str) {
    for entree in std::fs::read_dir(racine).into_iter().flatten().flatten() {
        if entree.path().is_dir() && entree.file_name() != garder {
            let _ = std::fs::remove_dir_all(entree.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convertit_les_champions() {
        let v: Value = serde_json::from_str(
            r#"{"data":{"MonkeyKing":{"key":"62","id":"MonkeyKing","name":"Wukong"}}}"#,
        )
        .unwrap();
        let lignes = tsv_champions(&v);
        assert_eq!(lignes, vec!["62\tMonkeyKing\tWukong"]);
        let (cle, reste) = lire_tsv(&lignes[0]).next().unwrap();
        assert_eq!((cle, reste), (62, "MonkeyKing\tWukong"));
    }

    #[test]
    fn convertit_les_runes() {
        let v: Value = serde_json::from_str(
            r#"[{"id":8100,"name":"Domination","slots":[{"runes":[{"id":8112,"name":"Électrocution"}]}]}]"#,
        )
        .unwrap();
        assert_eq!(tsv_runes(&v), vec!["8100\tDomination", "8112\tÉlectrocution"]);
    }

    #[test]
    fn classe_les_objets() {
        let v: Value = serde_json::from_str(
            r#"{"data":{
              "3020":{"name":"Chaussures du sorcier","gold":{"total":1100,"purchasable":true},"tags":["Boots"],
                      "from":["1001"],"into":["3175"],"depth":2,"maps":{"11":true}},
              "1001":{"name":"Bottes","gold":{"total":300,"purchasable":true},"tags":["Boots"],
                      "into":["3020"],"maps":{"11":true}},
              "3089":{"name":"Coiffe de Rabadon","gold":{"total":3600,"purchasable":true},"tags":["SpellDamage"],
                      "from":["1058"],"depth":3,"maps":{"11":true}},
              "3108":{"name":"Codex","gold":{"total":900,"purchasable":true},"into":["3089"],"maps":{"11":true}},
              "2003":{"name":"Potion","gold":{"total":50,"purchasable":true},"tags":["Consumable"],"maps":{"11":true}},
              "7002":{"name":"Objet d'Ornn","gold":{"total":3200,"purchasable":false},"requiredAlly":"Ornn","maps":{"11":true}},
              "443089":{"name":"Rabadon (Arena)","gold":{"total":3600,"purchasable":true},"maps":{"11":false,"30":true}},
              "1082":{"name":"Sceau noir","gold":{"total":350,"purchasable":true},"into":["3041"],"maps":{"11":true}},
              "3041":{"name":"Mejai","gold":{"total":1500,"purchasable":true},"from":["1082"],"maps":{"11":true}},
              "1055":{"name":"Lame de Doran","gold":{"total":450,"purchasable":true},"maps":{"11":true}},
              "3070":{"name":"Larme","gold":{"total":400,"purchasable":true},"into":["3004"],"maps":{"11":true}},
              "3004":{"name":"Manamune","gold":{"total":2900,"purchasable":true},"from":["3070"],"into":["3042"],"maps":{"11":true}},
              "3042":{"name":"Muramana","gold":{"total":2900,"purchasable":false},"specialRecipe":3004,"maps":{"11":true}}
            }}"#,
        )
        .unwrap();
        let lignes = tsv_objets(&v);
        assert_eq!(lignes[0], FORMAT_OBJETS);
        assert!(lignes.contains(&"3089\tCoiffe de Rabadon\t3600\t3\tFAD".to_string()), "{lignes:?}");
        let texte = lignes.join("\n");
        let genres: HashMap<u32, GenreObjet> =
            lire_tsv(&texte).filter_map(|(k, r)| Some((k, genre_objet_tsv(r)?))).collect();
        assert_eq!(genres.get(&3089), Some(&GenreObjet::Legendaire));
        assert_eq!(genres.get(&3020), Some(&GenreObjet::Bottes));
        // Mejai (< 2 000 PO), Manamune (son `into` ne s'achète pas) et Muramana (transformation).
        for id in [3041, 3004, 3042] {
            assert_eq!(genres.get(&id), Some(&GenreObjet::Legendaire), "objet {id}");
        }
        assert_eq!(genres.len(), 5, "ni Bottes de base, ni composant, ni potion, ni Doran, ni Ornn, ni Arena : {genres:?}");
        // Lecture unique du fichier : mêmes résultats.
        let (noms, genres_lus) = lire_objets(&texte);
        assert_eq!((noms.len(), genres_lus), (13, genres.clone()));
        // Le nom se lit pareil dans les deux formats.
        let nom = lire_tsv::<u32>(&texte).find(|(k, _)| *k == 3089).map(|(_, r)| nom_objet_tsv(r));
        assert_eq!(nom, Some("Coiffe de Rabadon"));
        assert_eq!(nom_objet_tsv("Ancien nom"), "Ancien nom");
        assert_eq!(genre_objet_tsv("Ancien nom"), None);
    }

    #[test]
    fn detecte_l_ancien_format_du_cache() {
        let racine = std::env::temp_dir().join(format!("stat-traque-test-dd-{}", std::process::id()));
        let dossier = racine.join("16.1.1");
        std::fs::create_dir_all(&dossier).unwrap();
        std::fs::write(dossier.join("objets.tsv"), "3089\tCoiffe").unwrap();
        assert!(complet(&racine, "16.1.1") && !a_jour(&racine, "16.1.1"));
        std::fs::write(dossier.join("objets.tsv"), format!("{FORMAT_OBJETS}\n3089\tCoiffe\t3600\t3\tFA")).unwrap();
        assert!(a_jour(&racine, "16.1.1"));
        let _ = std::fs::remove_dir_all(&racine);
    }
}
