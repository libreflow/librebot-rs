//! librebot-cmd : envoie une unique commande/message de chat en tant que le bot
//! puis se déconnecte. Utile quand le bot est opérateur du serveur.
//!
//! Exemple :
//!     librebot-cmd --host serveur.example --port 25565 "/kill @e[type=item]"

use librebot_rs::bot::{Bot, Event};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let mut host = "127.0.0.1".to_string();
    let mut port = 25565u16;
    let mut username = "LibreBot".to_string();
    let mut view = 8i8;
    let mut mots: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--host" => host = args.next().unwrap_or_default(),
            "--port" => port = args.next().and_then(|v| v.parse().ok()).unwrap_or(25565),
            "--username" => username = args.next().unwrap_or_default(),
            "--view" => view = args.next().and_then(|v| v.parse().ok()).unwrap_or(8),
            _ => mots.push(a),
        }
    }
    let message = mots.join(" ");
    if message.trim().is_empty() {
        eprintln!("usage : librebot-cmd [--host H] [--port P] [--username U] \"<message ou /commande>\"");
        std::process::exit(2);
    }
    println!("connexion à {}:{} en tant que {}…", host, port, username);
    let mut bot = match Bot::connect_with_view(&host, port, &username, view).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("connexion impossible : {}", e);
            std::process::exit(1);
        }
    };
    // Attendre d'être dans le monde (login puis position)
    let _ = bot.play_turn().await; // Connected (login)
    let debut = std::time::Instant::now();
    while debut.elapsed() < Duration::from_secs(30) {
        match tokio::time::timeout(Duration::from_millis(500), bot.play_turn()).await {
            Ok(Ok(Event::Spawned)) => break,
            Ok(Ok(Event::Disconnected(r))) => {
                eprintln!("déconnecté : {}", r);
                std::process::exit(1);
            }
            Ok(Err(e)) => {
                eprintln!("erreur de protocole : {}", e);
                std::process::exit(1);
            }
            _ => {}
        }
        if bot.pos.is_some() {
            break;
        }
    }
    if let Err(e) = bot.say(&message).await {
        eprintln!("envoi impossible : {}", e);
        std::process::exit(1);
    }
    println!("envoyé : {}", message);
    // Laisser le serveur traiter la commande avant de couper
    tokio::time::sleep(Duration::from_secs(3)).await;
}
