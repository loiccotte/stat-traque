//! Les deux clients HTTP de l'app, créés UNE fois et réutilisés partout
//! (réutiliser un client garde les connexions ouvertes : plus rapide).
//!
//! - `local()`  : pour le client LoL et le jeu (127.0.0.1). Ils utilisent un
//!   certificat "fait maison", on l'accepte. Sans danger : on ne parle qu'à notre PC.
//! - `internet()` : pour Riot et Data Dragon, avec vérification normale des certificats.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Connecteur TLS qui accepte les certificats du client LoL (réutilisé aussi par le WebSocket).
pub fn tls_local() -> native_tls::TlsConnector {
    native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("TLS indisponible")
}

pub fn local() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .tls_connector(Arc::new(tls_local()))
            .timeout(Duration::from_secs(2))
            .build()
    })
}

pub fn internet() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let tls = native_tls::TlsConnector::new().expect("TLS indisponible");
        ureq::AgentBuilder::new()
            .tls_connector(Arc::new(tls))
            .timeout(Duration::from_secs(5))
            .user_agent("stat-traque")
            .build()
    })
}
