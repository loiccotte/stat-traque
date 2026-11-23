//! Les phases du client LoL (`/lol-gameflow/v1/gameflow-phase`).
//! On les traduit en `enum` : le compilateur nous oblige alors à traiter tous les cas.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Phase {
    Aucune,
    Lobby,
    Matchmaking,
    ReadyCheck,
    ChampSelect,
    GameStart,
    InProgress,
    Reconnect,
    WaitingForStats,
    PreEndOfGame,
    EndOfGame,
    Autre,
}

impl Phase {
    /// Traduit le texte envoyé par le client (ex : "ChampSelect").
    pub fn depuis_texte(s: &str) -> Self {
        match s {
            "None" => Phase::Aucune,
            "Lobby" => Phase::Lobby,
            "Matchmaking" => Phase::Matchmaking,
            "ReadyCheck" => Phase::ReadyCheck,
            "ChampSelect" => Phase::ChampSelect,
            "GameStart" => Phase::GameStart,
            "InProgress" => Phase::InProgress,
            "Reconnect" => Phase::Reconnect,
            "WaitingForStats" => Phase::WaitingForStats,
            "PreEndOfGame" => Phase::PreEndOfGame,
            "EndOfGame" => Phase::EndOfGame,
            _ => Phase::Autre,
        }
    }

    /// Le jeu tourne-t-il ? (c'est seulement là qu'on interroge le port 2999)
    pub fn en_partie(self) -> bool {
        matches!(self, Phase::InProgress | Phase::GameStart | Phase::Reconnect)
    }

    /// Libellé court pour l'icône et le journal.
    pub fn libelle(self) -> &'static str {
        match self {
            Phase::Aucune => "Au repos",
            Phase::Autre => "Autre",
            Phase::Lobby => "Salon",
            Phase::Matchmaking => "Recherche de partie",
            Phase::ReadyCheck => "Partie trouvée",
            Phase::ChampSelect => "Champ select",
            Phase::GameStart | Phase::InProgress | Phase::Reconnect => "En partie",
            Phase::WaitingForStats | Phase::PreEndOfGame | Phase::EndOfGame => "Fin de partie",
        }
    }
}

/// File (queueId) de la partie d'après `/lol-gameflow/v1/session` du client :
/// `gameData.queue.id`. None pour une partie personnalisée, l'outil d'entraînement
/// (id ≤ 0) ou une réponse inattendue. Source LOCALE : marche même si spectator échoue.
pub fn file_depuis_session(session: &serde_json::Value) -> Option<u16> {
    let donnees = &session["gameData"];
    // Partie personnalisée, ou TFT (pas une partie LoL : jamais dans match-v5 LoL).
    if donnees["isCustomGame"] == true || donnees["queue"]["gameMode"] == "TFT" {
        return None;
    }
    donnees["queue"]["id"].as_i64().filter(|id| *id > 0 && *id <= u16::MAX as i64).map(|id| id as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_la_file_de_la_session() {
        let v = |t: &str| serde_json::from_str::<serde_json::Value>(t).unwrap();
        assert_eq!(file_depuis_session(&v(r#"{"gameData":{"queue":{"id":1750,"gameMode":"CHERRY"}}}"#)), Some(1750));
        assert_eq!(file_depuis_session(&v(r#"{"gameData":{"queue":{"id":-1}}}"#)), None);
        assert_eq!(file_depuis_session(&v(r#"{"gameData":{"isCustomGame":true,"queue":{"id":0}}}"#)), None);
        assert_eq!(file_depuis_session(&v("{}")), None);
        assert_eq!(file_depuis_session(&v(r#"{"gameData":{"queue":{"id":1100,"gameMode":"TFT"}}}"#)), None);
    }

    #[test]
    fn traduit_les_phases() {
        assert_eq!(Phase::depuis_texte("ChampSelect"), Phase::ChampSelect);
        assert_eq!(Phase::depuis_texte("truc"), Phase::Autre);
        assert!(Phase::depuis_texte("InProgress").en_partie());
        assert!(!Phase::Lobby.en_partie());
    }
}
