//! Verrou anti-double-instance : fichier lock créé atomiquement (create_new).
//! Si le fichier existe et que le PID inscrit est mort, on le réclame.

use std::fs::OpenOptions;
use std::io::Write;

const CHEMIN_LOCK: &str = "librebot.lock";

/// Tente de prendre le verrou ; Err(()) si une autre instance vit.
pub fn prendre_verrou() -> Result<(), String> {
    if let Ok(contenu) = std::fs::read_to_string(CHEMIN_LOCK) {
        if let Ok(pid) = contenu.trim().parse::<u32>() {
            if pid_mort(pid) {
                let _ = std::fs::remove_file(CHEMIN_LOCK); //.instance morte : réclamer
            } else {
                return Err(format!("une autre instance vit (PID {})", pid));
            }
        } else {
            let _ = std::fs::remove_file(CHEMIN_LOCK);
        }
    }
    match OpenOptions::new().write(true).create_new(true).open(CHEMIN_LOCK) {
        Ok(mut f) => {
            let _ = write!(f, "{}", std::process::id());
            Ok(())
        }
        Err(_) => Err("verrou déjà pris".into()),
    }
}

pub fn liberer_verrou() {
    let _ = std::fs::remove_file(CHEMIN_LOCK);
}

#[cfg(unix)]
fn pid_mort(pid: u32) -> bool {
    // kill(0) : teste l'existence sans envoyer de signal.
    !std::path::Path::new("/proc").join(pid.to_string()).exists()
        && unsafe { libc_kill_zero(pid) != 0 }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill_zero(pid: u32) -> i32;
}

#[cfg(not(unix))]
fn pid_mort(_pid: u32) -> bool {
    true
}
