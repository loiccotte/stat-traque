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
    #[allow(dead_code)] // utilisé par la partie joueur (sorts auto)
    pub sorts: HashMap<u16, Box<str>>,
    pub runes: HashMap<u16, Box<str>>,
    pub objets: HashMap<u16, Box<str>>,
}

impl DonneesJeu {
    pub fn nom_champion(&self, id: u16) -> &str {
        self.champions.get(&id).map_or("inconnu", |c| &c.nom)
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
                        "Data Dragon {} : {} champions, {} runes, {} objets ({} ms)",
                        d.version,
                        d.champions.len(),
                        d.runes.len(),
                        d.objets.len(),
                        debut.elapsed().as_millis()
                    );
                    let _ = tx.send(Evenement::DonneesPretes(Arc::new(d)));
                }
                None => log!("Data Dragon indisponible (pas d'internet et pas de cache)."),
            }
        })
        .expect("impossible de lancer le thread Data Dragon");
}

/// Version la plus récente : demandée à Riot, sinon la dernière connue (hors ligne).
fn version_actuelle(dossier: &Path) -> Option<String> {
    let fichier = dossier.join("version.txt");
    let en_ligne = crate::http::internet()
        .get(&format!("{CDN}/api/versions.json"))
        .call()
        .ok()
        .and_then(|r| r.into_json::<Value>().ok())
        .and_then(|v| v[0].as_str().map(String::from));
    match en_ligne {
        Some(v) => {
            let _ = std::fs::write(&fichier, &v);
            Some(v)
        }
        None => std::fs::read_to_string(fichier).ok().map(|s| s.trim().to_string()),
    }
}

pub fn charger() -> Option<DonneesJeu> {
    let racine = crate::stockage::dossier_cache("ddragon");
    let version = version_actuelle(&racine)?;
    let dossier = racine.join(&version);
    if !dossier.join("objets.tsv").exists() {
        log!("Nouveau patch {version} : téléchargement des données...");
        telecharger(&version, &dossier)?;
        nettoyer_anciens_patchs(&racine, &version);
    }
    let lire = |nom: &str| std::fs::read_to_string(dossier.join(nom)).unwrap_or_default();
    Some(DonneesJeu {
        champions: lire_tsv(&lire("champions.tsv"))
            .map(|(cle, reste)| {
                let (id, nom) = reste.split_once('\t').unwrap_or((reste, reste));
                (cle, Champion { id: id.into(), nom: nom.into() })
            })
            .collect(),
        sorts: lire_tsv(&lire("sorts.tsv")).map(|(k, v)| (k, v.into())).collect(),
        runes: lire_tsv(&lire("runes.tsv")).map(|(k, v)| (k, v.into())).collect(),
        objets: lire_tsv(&lire("objets.tsv")).map(|(k, v)| (k, v.into())).collect(),
        version,
    })
}

/// Lit des lignes `id<TAB>reste`.
fn lire_tsv(texte: &str) -> impl Iterator<Item = (u16, &str)> {
    texte.lines().filter_map(|l| {
        let (id, reste) = l.split_once('\t')?;
        Some((id.parse().ok()?, reste))
    })
}

fn json(url: &str) -> Option<Value> {
    crate::http::internet().get(url).call().ok()?.into_json().ok()
}

/// Télécharge les 4 fichiers du patch et les convertit en petits TSV.
fn telecharger(version: &str, dossier: &Path) -> Option<()> {
    let base = format!("{CDN}/cdn/{version}/data/{LANGUE}");
    let champions = json(&format!("{base}/champion.json"))?;
    let sorts = json(&format!("{base}/summoner.json"))?;
    let runes = json(&format!("{base}/runesReforged.json"))?;
    let objets = json(&format!("{base}/item.json"))?;

    std::fs::create_dir_all(dossier).ok()?;
    let ecrire = |nom: &str, lignes: Vec<String>| std::fs::write(dossier.join(nom), lignes.join("\n"));
    ecrire("champions.tsv", tsv_champions(&champions)).ok()?;
    ecrire("sorts.tsv", tsv_par_cle(&sorts)).ok()?;
    ecrire("runes.tsv", tsv_runes(&runes)).ok()?;
    // objets.tsv en dernier : sa présence signifie "patch complet".
    ecrire("objets.tsv", tsv_objets(&objets)).ok()?;
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

/// item.json : {"data": {"3020": {"name": "..."}}} (l'id est la clé).
fn tsv_objets(v: &Value) -> Vec<String> {
    v["data"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, o)| Some(format!("{id}\t{}", o["name"].as_str()?)))
        .collect()
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
}
