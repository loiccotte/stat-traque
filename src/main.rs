// stat-traque : petit outil qui parle au client League of Legends.
//
// Principe général (à retenir) :
//  1. Le client LoL ouvre un petit serveur web local (https://127.0.0.1:PORT).
//  2. Le PORT et un MOT DE PASSE sont donnés dans les arguments du processus
//     "LeagueClientUx.exe". On les lit pour pouvoir lui parler.
//  3. On pose des questions à ce serveur ("quel champion j'ai pris ?") en boucle.
//
// En Rust :  `let x = ...`  crée une variable,  `fn` définit une fonction,
//  `Option<T>` = "il y a une valeur OU rien",  `Result<T, E>` = "ça a marché OU erreur".
//  Le `?` à la fin d'une ligne veut dire "si ça a raté, arrête la fonction et renvoie l'erreur".

use std::collections::HashMap;
use std::process::Command;
use std::{thread, time::Duration};

use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::blocking::Client;
use serde_json::Value;

/// Les infos pour se connecter au client LoL.
struct Lcu {
    port: String,
    token: String,
}

/// Étape 1 : trouver le port et le mot de passe.
/// Le client LoL écrit un petit fichier "lockfile" dans son dossier d'installation.
/// Il contient une seule ligne : nom:pid:PORT:TOKEN:protocole
/// Renvoie `None` si le client n'est pas lancé (pas de lockfile).
fn trouver_client() -> Option<Lcu> {
    // 1) Où est installé le jeu ? On demande d'abord au processus lancé,
    //    sinon on essaie les emplacements habituels.
    let mut dossiers: Vec<String> = Vec::new();
    if let Ok(sortie) = Command::new("powershell")
        .args(["-NoProfile", "-Command", "(Get-Process LeagueClientUx -ErrorAction SilentlyContinue).Path"])
        .output()
    {
        let chemin = String::from_utf8_lossy(&sortie.stdout).trim().to_string();
        if let Some(dossier) = std::path::Path::new(&chemin).parent() {
            dossiers.push(dossier.to_string_lossy().to_string());
        }
    }
    for lettre in ["C", "D", "E"] {
        dossiers.push(format!("{lettre}:\\Riot Games\\League of Legends"));
    }

    // 2) On lit le premier lockfile qu'on trouve et on le découpe sur les ":".
    for dossier in dossiers {
        let Ok(contenu) = std::fs::read_to_string(format!("{dossier}\\lockfile")) else {
            continue;
        };
        let parties: Vec<&str> = contenu.trim().split(':').collect();
        if parties.len() >= 4 {
            return Some(Lcu { port: parties[2].to_string(), token: parties[3].to_string() });
        }
    }
    None
}

/// Pose une question au client LoL et renvoie la réponse en JSON.
/// `chemin` ressemble à "/lol-champ-select/v1/session".
fn demander(client: &Client, lcu: &Lcu, chemin: &str) -> Option<Value> {
    let url = format!("https://127.0.0.1:{}{}", lcu.port, chemin);
    // Le mot de passe se fait avec l'utilisateur "riot" + le token, encodés en base64.
    let auth = STANDARD.encode(format!("riot:{}", lcu.token));
    let rep = client
        .get(url)
        .header("Authorization", format!("Basic {auth}"))
        .send()
        .ok()?;
    if !rep.status().is_success() {
        return None; // ex : pas en champ select -> le client répond une erreur
    }
    rep.json().ok()
}

/// Récupère la liste "numéro de champion -> nom" depuis Data Dragon (site public de Riot).
/// Le client ne nous donne qu'un numéro (ex : 103), on veut "Ahri".
fn charger_noms_champions() -> HashMap<i64, String> {
    let mut noms = HashMap::new();
    let client = Client::new();
    let versions: Option<Value> = client
        .get("https://ddragon.leagueoflegends.com/api/versions.json")
        .send()
        .ok()
        .and_then(|r| r.json().ok());
    let Some(version) = versions.and_then(|v| v[0].as_str().map(String::from)) else {
        return noms;
    };
    let url = format!(
        "https://ddragon.leagueoflegends.com/cdn/{version}/data/fr_FR/champion.json"
    );
    let data: Option<Value> = client.get(url).send().ok().and_then(|r| r.json().ok());
    if let Some(champs) = data.as_ref().and_then(|d| d["data"].as_object()) {
        for (_, c) in champs {
            if let (Some(cle), Some(nom)) = (c["key"].as_str(), c["name"].as_str()) {
                if let Ok(id) = cle.parse::<i64>() {
                    noms.insert(id, nom.to_string());
                }
            }
        }
    }
    noms
}

/// Pendant une partie, le jeu lui-même ouvre une API sur le port 2999 (sans mot de passe).
/// Elle donne les 10 joueurs, leurs champions, leur KDA, leurs items...
/// Renvoie `None` si aucune partie n'est en cours.
fn partie_en_cours(client: &Client) -> Option<Value> {
    let rep = client
        .get("https://127.0.0.1:2999/liveclientdata/allgamedata")
        .send()
        .ok()?;
    rep.json().ok()
}

/// Affiche le tableau des joueurs de la partie.
fn afficher_partie(partie: &Value) {
    let mode = partie["gameData"]["gameMode"].as_str().unwrap_or("?");
    let temps = partie["gameData"]["gameTime"].as_f64().unwrap_or(0.0) as i64;
    println!("\n--- Partie {mode} | {}:{:02} ---", temps / 60, temps % 60);
    if let Some(joueurs) = partie["allPlayers"].as_array() {
        for j in joueurs {
            let champion = j["championName"].as_str().unwrap_or("?");
            let pseudo = j["riotIdGameName"].as_str().unwrap_or("?");
            let equipe = j["team"].as_str().unwrap_or("?");
            let s = &j["scores"];
            println!(
                "{equipe:<6} {champion:<14} {}/{}/{}  {pseudo}",
                s["kills"], s["deaths"], s["assists"]
            );
        }
    }
}

fn main() {
    println!("stat-traque : en attente du client League of Legends...");

    // Le client LoL utilise un certificat "fait maison" : on dit à reqwest de l'accepter.
    // (C'est sans danger ici car on ne parle qu'à notre propre PC, 127.0.0.1.)
    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(3))
        .build()
        .expect("impossible de créer le client HTTP");

    let noms = charger_noms_champions();
    println!("{} champions chargés.", noms.len());

    let mut lcu: Option<Lcu> = None;
    let mut dernier_champion: i64 = 0;
    let mut tour: u32 = 0; // compte les secondes écoulées

    // Boucle infinie : toutes les secondes on regarde où on en est.
    loop {
        // Si on n'est pas connecté, on essaie de trouver le client.
        if lcu.is_none() {
            lcu = trouver_client();
            if lcu.is_some() {
                println!("Client LoL trouvé et connecté.");
            }
        }

        if let Some(l) = &lcu {
            // "current-champion" renvoie le numéro du champion qu'on survole/verrouille.
            match demander(&client, l, "/lol-champ-select/v1/current-champion") {
                Some(v) => {
                    let id = v.as_i64().unwrap_or(0);
                    if id != 0 && id != dernier_champion {
                        let nom = noms.get(&id).map(String::as_str).unwrap_or("inconnu");
                        println!("Champion sélectionné : {nom} (id {id})");
                        dernier_champion = id;
                    }
                }
                None => {
                    // Pas en champ select (ou client fermé).
                    dernier_champion = 0;
                    // On vérifie que le client est toujours là, sinon on se déconnecte.
                    if demander(&client, l, "/lol-summoner/v1/current-summoner").is_none() {
                        println!("Client LoL perdu, nouvelle recherche...");
                        lcu = None;
                    }
                }
            }
        }

        // Toutes les 10 secondes, si une partie est en cours, on affiche le tableau.
        tour += 1;
        if tour % 10 == 0 {
            if let Some(partie) = partie_en_cours(&client) {
                afficher_partie(&partie);
            }
        }

        thread::sleep(Duration::from_secs(1));
    }
}
