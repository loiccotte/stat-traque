//! Les phases du client LoL (`/lol-gameflow/v1/gameflow-phase`).
//! On les traduit en `enum` : le compilateur nous oblige alors à traiter tous les cas.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            Phase::Aucune | Phase::Autre => "Au repos",
            Phase::Lobby => "Salon",
            Phase::Matchmaking => "Recherche de partie",
            Phase::ReadyCheck => "Partie trouvée",
            Phase::ChampSelect => "Champ select",
            Phase::GameStart | Phase::InProgress | Phase::Reconnect => "En partie",
            Phase::WaitingForStats | Phase::PreEndOfGame | Phase::EndOfGame => "Fin de partie",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traduit_les_phases() {
        assert_eq!(Phase::depuis_texte("ChampSelect"), Phase::ChampSelect);
        assert_eq!(Phase::depuis_texte("truc"), Phase::Autre);
        assert!(Phase::depuis_texte("InProgress").en_partie());
        assert!(!Phase::Lobby.en_partie());
    }
}
