//! Détection de version par ping de statut (handshake next_state=1).
//! Zéro LLM : simple lecture du JSON vanilla. Seul 767 (1.21.1) est
//! implémenté ; toute autre version produit un message explicite.

use crate::protocol::{write_string, write_varint, Conn, ReadOutcome};
use std::time::Duration;

/// Protocole réellement implémenté par ce client.
pub const PROTOCOLE_SUPPORTÉ: i32 = 767;

#[derive(Debug, Clone)]
pub struct InfoServeur {
    pub protocole: i32,
    pub version: String,
    pub joueurs: i64,
}

/// Ping de statut : renvoie la version affichée + protocole + joueurs.
pub async fn ping_version(host: &str, port: u16) -> std::io::Result<InfoServeur> {
    let stream = tokio::time::timeout(
        Duration::from_secs(8),
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout connexion"))??;
    let mut conn = Conn::new(stream);

    // Handshake intention=1 (statut)
    let mut b = Vec::new();
    write_varint(0, &mut b); // id handshake
    write_varint(PROTOCOLE_SUPPORTÉ, &mut b);
    write_string(host, &mut b);
    b.extend_from_slice(&port.to_be_bytes());
    write_varint(1, &mut b); // statut
    conn.send(0x00, &b).await?;

    // Ping request
    conn.send(0x00, &[]).await?;

    let reponse = loop {
        match conn.read_packet().await? {
            ReadOutcome::Packet(0x00, json) => break json,
            ReadOutcome::Eof => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "connexion fermée pendant le ping",
                ))
            }
            ReadOutcome::Packet(_, _) => continue,
        }
    };
    let texte = String::from_utf8_lossy(&reponse).into_owned();
    let v = parse_version(&texte).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "réponse de statut illisible")
    })?;
    Ok(v)
}

fn parse_version(json: &str) -> Option<InfoServeur> {
    let debut = json.find("{\"version\"")?;
    let v = &json[debut..];
    let proto = v
        .split("\"protocol\":")
        .nth(1)?
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .next()?
        .parse::<i32>()
        .ok()?;
    let nom = v
        .split("\"name\":\"")
        .nth(1)?
        .split('"')
        .next()?
        .to_string();
    let joueurs = v
        .split("\"online\":")
        .nth(1)?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse::<i64>()
        .unwrap_or(0);
    Some(InfoServeur {
        protocole: proto,
        version: nom,
        joueurs,
    })
}

/// Message explicite quand la version détectée n'est pas supportée.
pub fn message_non_supporte(info: &InfoServeur) -> String {
    format!(
        "serveur \"{}\" (protocole {}) non supporté : ce client implémente uniquement \
         le protocole 767 (Minecraft 1.21.1). Sur Aternos, sélectionne la version \
         1.21.1 dans le panneau ; sinon utilise un autre client.",
        info.version, info.protocole
    )
}
