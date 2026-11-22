//! librebot-rs : bot Minecraft Java Edition (client protocole natif Rust).

use librebot_rs::bot::{Bot, Event};
use librebot_rs::config::Config;
use librebot_rs::protocol::PROTOCOL_VERSION;
use std::path::PathBuf;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let mut host = None;
    let mut port = None;
    let mut username = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--host" => host = args.next(),
            "--port" => port = args.next().and_then(|v| v.parse().ok()),
            "--username" => username = args.next(),
            _ => {}
        }
    }
    let cfg_path = PathBuf::from("config.json");
    let cfg = Config::load(&cfg_path, host.as_deref(), port, username.as_deref());

    println!(
        "librebot-rs (protocole {}) — cible {}:{} en tant que {}",
        PROTOCOL_VERSION, cfg.host, cfg.port, cfg.username
    );
    loop {
        match Bot::connect(&cfg.host, cfg.port, &cfg.username).await {
            Ok(bot) => session(bot).await,
            Err(e) => eprintln!("connexion impossible : {}", e),
        }
        println!(
            "reconnexion dans {} s (Ctrl-C pour arrêter)",
            cfg.reconnect_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(cfg.reconnect_secs)).await;
    }
}

async fn session(bot: Bot) {
    let (tx, mut rx) = mpsc::channel::<String>(16);
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        let reader = std::io::BufReader::new(stdin.lock());
        for line in reader.lines() {
            match line {
                Ok(l) if !l.trim().is_empty() => {
                    if tx.blocking_send(l.trim().to_string()).is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    });

    let mut bot = bot;
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            ev = bot.play_turn() => match ev {
                Ok(Event::Connected) => {
                    println!("[{}] dans le monde {} (entité {:?})", bot.username, bot.dimension.as_deref().unwrap_or("?"), bot.entity_id);
                    // Lancer la mission survie automatiquement
                    println!("--- Démarrage de la mission survie ---");
                    let r1 = bot.phase_wood().await;
                    println!("Phase bois: {}", if r1 { "OK" } else { "ÉCHEC" });
                    if r1 {
                        let r2 = bot.phase_stone().await;
                        println!("Phase pierre: {}", if r2 { "OK" } else { "ÉCHEC" });
                        if r2 {
                            let r3 = bot.phase_iron().await;
                            println!("Phase fer: {}", if r3 { "OK" } else { "ÉCHEC" });
                        }
                    }
                    println!("--- Mission terminée, bot en attente de commandes ---");
                }
                Ok(Event::Spawned) => {
                    if let Some(p) = bot.pos {
                        println!("apparition en ({:.2}, {:.2}, {:.2})", p[0], p[1], p[2]);
                    }
                }
                Ok(Event::Chat(t)) => println!("{}", t),
                Ok(Event::Died) => println!("mort détectée (vie={:.1}) — respawn demandé au serveur", bot.health),
                Ok(Event::Disconnected(r)) => { println!("déconnecté : {}", r); break; }
                Err(e) => { eprintln!("erreur de protocole : {}", e); break; }
            },
            Some(msg) = rx.recv() => {
                if let Some(rest) = msg.strip_prefix("goto ") {
                    let coords: Vec<i32> = rest.split_whitespace().filter_map(|w| w.parse().ok()).collect();
                    if coords.len() == 3 {
                        match bot.goto(coords[0], coords[1], coords[2]).await {
                            Some(n) => println!("chemin de {} étapes vers ({}, {}, {})", n, coords[0], coords[1], coords[2]),
                            None => println!("aucun chemin trouvé vers ({}, {}, {})", coords[0], coords[1], coords[2]),
                        }
                    } else {
                        println!("usage : goto <x> <y> <z>");
                    }
                } else if let Err(e) = bot.say(&msg).await {
                    eprintln!("envoi impossible : {}", e);
                    break;
                }
            }
            _ = tick.tick() => {
                if let Err(e) = bot.tick(0.05).await {
                    eprintln!("erreur de physique : {}", e);
                    break;
                }
            }
        }
    }
}
