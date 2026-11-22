//! Watchdog externe : surveille le log du bot (bot.log) ; s'il n'a rien
//! écrit depuis 2 min 30, tue le processus librebot et le relance.
//! Usage : `watchdog <chemin/vers/log> <script_de_lancement>` (optionnel).
//! Sans arguments : surveille ./bot.log et relance ./run.sh s'il existe.

use std::process::Command;
use std::time::{Duration, SystemTime};

const SEUIL_SILENCE: Duration = Duration::from_secs(150);

fn main() {
    let args: Vec<String> = std::env::args().collect::<Vec<_>>().into_iter().skip(1).collect();
    let log = args.clone().into_iter().next().unwrap_or_else(|| "bot.log".into());
    let relance = args.into_iter().nth(1).unwrap_or_else(|| "run.sh".into());

    println!("watchdog : surveillance de {} (silence > {:?})", log, SEUIL_SILENCE);
    loop {
        let silence = dernier_ecrit(&log)
            .map(|t| SystemTime::now().duration_since(t).unwrap_or(Duration::ZERO))
            .unwrap_or(Duration::from_secs(3600));
        if silence > SEUIL_SILENCE {
            println!("watchdog : bot muet depuis {:?} — relance", silence);
            tuer_librebot();
            let _ = Command::new("sh").arg(&relance).spawn();
            std::thread::sleep(Duration::from_secs(20));
            continue;
        }
        std::thread::sleep(Duration::from_secs(30));
    }
}

fn dernier_ecrit(chemin: &str) -> Option<SystemTime> {
    std::fs::metadata(chemin).ok()?.modified().ok()
}

fn tuer_librebot() {
    let _ = Command::new("pkill").arg("-9").arg("-x").arg("librebot").status();
}
