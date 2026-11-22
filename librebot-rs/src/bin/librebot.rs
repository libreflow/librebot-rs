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

    // Verrou anti-double-instance : un seul bot par répertoire.
    if let Err(e) = librebot_rs::verrou::prendre_verrou() {
        eprintln!("refus de démarrer : {}", e);
        return;
    }

    // Détection de version (config version: "auto") : ping de statut et
    // message explicite si le protocole n'est pas le 767 implémenté.
    if cfg.version.as_deref().map(|v| v == "auto").unwrap_or(true) {
        match librebot_rs::version::ping_version(&cfg.host, cfg.port).await {
            Ok(info) => {
                println!("cible : {} (protocole {}, {} joueur(s))",
                    info.version, info.protocole, info.joueurs);
                if info.protocole != librebot_rs::version::PROTOCOLE_SUPPORTÉ {
                    eprintln!("{}", librebot_rs::version::message_non_supporte(&info));
                    librebot_rs::verrou::liberer_verrou();
                    return;
                }
            }
            Err(e) => {
                eprintln!(
                    "ping de statut impossible ({}) — le serveur dort probablement ; reconnexion en boucle.",
                    e
                );
            }
        }
    }

    println!(
        "librebot-rs (protocole {}) — cible {}:{} en tant que {}",
        PROTOCOL_VERSION, cfg.host, cfg.port, cfg.username
    );
    // Dispersion au démarrage : les instances d'une flotte ne doivent pas
    // ouvrir leur connexion à la même seconde (Paper throttle ~1/IP/4 s).
    if cfg.start_jitter_secs > 0 {
        let nano = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        let attente_ms = nano % (cfg.start_jitter_secs * 1000 + 1);
        println!("démarrage décalé de {} ms (dispersion de flotte)", attente_ms);
        tokio::time::sleep(std::time::Duration::from_millis(attente_ms)).await;
    }
    // Backoff exponentiel : Aternos throttle les reconnexions rapides en
    // fermant les connexions suivantes — doubler le délai à chaque échec
    // consécutif (jusqu'à 5 min) rend le bot fiable sur les hébergements free.
    // Exception : un « Connection throttled » explicite n'est pas un échec
    // du bot — attendre quelques secondes suffit, pas un doublement.
    let mut attente = cfg.reconnect_secs;
    let mut throttle_repete = 0u32;
    loop {
        let debut = std::time::Instant::now();
        let mut est_throttle = false;
        match Bot::connect_with_view(&cfg.host, cfg.port, &cfg.username, cfg.view_distance as i8).await {
            Ok(bot) => {
                // Panics confinées par session : une erreur ne tue jamais le process.
                let tache = tokio::spawn(session(bot));
                if let Err(e) = tache.await {
                    if e.is_panic() {
                        eprintln!("panic confinée par tâche : {:?}", e);
                    }
                }
            }
            Err(e) => {
                est_throttle = e.to_string().to_lowercase().contains("throttled");
                eprintln!("connexion impossible : {}", e);
            }
        }
        let duree_session = debut.elapsed();
        if duree_session >= std::time::Duration::from_secs(120) {
            attente = cfg.reconnect_secs; // session saine : raz du backoff
            throttle_repete = 0;
        } else {
            // Throttle explicite du serveur : patience courte et fixe.
            if est_throttle {
                throttle_repete += 1;
                attente = 15 + (throttle_repete.min(4) * 5) as u64;
            } else {
                attente = (attente.saturating_mul(2)).min(300u64);
            }
        }
        println!(
            "reconnexion dans {} s (session écoulée {:.0} s — Ctrl-C pour arrêter)",
            attente,
            duree_session.as_secs_f64()
        );
        tokio::time::sleep(std::time::Duration::from_secs(attente)).await;
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
    let mut derniere_apparition: Option<[f64; 3]> = None;
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    enum Tour {
        Evenement(std::io::Result<Event>),
        Message(String),
        Physique,
    }
    loop {
        // 1) Envoyer les paquets différés : aucune écriture réseau n'est annulable.
        if let Err(e) = bot.flush_pending().await {
            eprintln!("erreur de protocole : {}", e);
            break;
        }
        // 2) Sélection : seule la lecture est un future longévif ; les actions I/O
        //    ont lieu après le select, sans jamais être annulées en cours d'envoi.
        let tour = tokio::select! {
            ev = bot.play_turn() => Tour::Evenement(ev),
            Some(msg) = rx.recv() => Tour::Message(msg),
            _ = tick.tick() => Tour::Physique,
        };
        match tour {
            Tour::Physique => bot.physics_tick(0.05),
            Tour::Message(msg) => {
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
            Tour::Evenement(ev) => match ev {
                Ok(Event::Connected) => {
                    println!("[{}] dans le monde {} (entité {:?})", bot.username, bot.dimension.as_deref().unwrap_or("?"), bot.entity_id);
                    if let Some(p) = bot.pos {
                        println!("position du bot : ({:.0}, {:.0}, {:.0}) — /teleport {} pour le retrouver", p[0], p[1], p[2], bot.username);
                    }
                    // Mission survie autonome : boucle infinie gérée par le module survie
                    println!("--- Démarrage de la mission survie autonome ---");
                    bot.run_mission().await;
                    println!("--- Mission arrêtée ---");
                }
                Ok(Event::Spawned) => {
                    // Le serveur renvoie ce paquet à chaque téléport corrective :
                    // n'afficher que les changements réels de position.
                    let affiche = match (bot.pos, derniere_apparition) {
                        (Some(p), Some(last)) => {
                            (p[0] - last[0]).abs() > 1.0 || (p[2] - last[2]).abs() > 1.0
                        }
                        (Some(_), None) => true,
                        _ => false,
                    };
                    if affiche {
                        if let Some(p) = bot.pos {
                            println!("apparition en ({:.2}, {:.2}, {:.2})", p[0], p[1], p[2]);
                            derniere_apparition = Some(p);
                        }
                    }
                }
                Ok(Event::Chat(t)) => {
                    println!("{}", t);
                    // Commandes chat joueur → bot (« LibreBot stop", "!statut"…)
                    bot.traiter_commande_chat(&t).await;
                }
                Ok(Event::Disconnected(r)) => { println!("déconnecté : {}", r); break; }
                Err(e) => { eprintln!("erreur de protocole : {}", e); break; }
            },
        }
    }
}
