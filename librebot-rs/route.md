# route.md — Contrat de route RSG (Random Seed Glitchless) 1.21.1

Ce fichier est le **contrat** entre les phases du speedrun. La machine à états
(`src/speedrun.rs`) doit s'y conformer : transitions explicites, plans B définis,
aucun comportement aléatoire, aucune panic.

## 1. Phases et splits (RunLogger, target `tracing` = `run`)

T0 = réception du paquet `login` (jeu), i.e. `Event::Connected`.

| # | Phase | Split (logger) | Contrat de sortie (vers la phase suivante) |
|---|---|---|---|
| 1 | `Spawn` | `spawn_ready` | ≥ 3 bûches, table de craft posée/ouverte, outils bois puis **pierre** (pioche pierre min). Réutilise `phase_wood` + `phase_stone`. |
| 2 | `NetherEntry` | `nether_entry` | Portail du Nether activé et traversé (dimension = `minecraft:the_nether`). Portail ruiné prioritaire, sinon construction (obsidienne + briquet). |
| 3 | `Bastion` | `bastion_done` | Loot réel du bastion évalue : perles (barter piglins), obsidienne, cordes, or. **Règle d'abandon** : si aucun perle après avoir vidé les coffres centraux → abandonner le bastion (event `PlanB`) et passer à la forteresse. |
| 4 | `Fortress` | `fortress_done` | ≥ **6 blaze rods** (12 poudres de blaze = 12 yeux), objectif ≥ 10 rods. |
| 5 | `Stronghold` | `stronghold_located` | Position du stronghold estimée par **triangulation ≥ 2 lancers d'yeux**. Si les deux yeux divergent (> 5° d'écart) → recalculer (3e lancer). |
| 6 | `End` | `dragon_dead` | Portail de l'End rempli et activé, Ender Dragon tué. **Fin du run.** |

### Contrat transverse Nether → Overworld (sortie du Nether)
- **≥ 8 perles de l'Ender** en inventaire à la sortie du Nether (12 yeux nécessaires,
  moins les yeux déjà présents dans le portail) → route normale.
- **Sinon plan B « mini-yeux »** : crafter les yeux avec les perles disponibles et
  compléter en explorant (event `PlanB`). Jamais de panic, jamais de reset aléatoire.

## 2. Gestion d'incidents (événements loggés, réactions explicites)

| Incident | Réaction |
|---|---|
| Mort (`health <= 0`, `Event::Died`) | `send_respawn`, event `Death` + `Respawn`, retour au **checkpoint** de la phase courante, re-split de la phase. |
| Blaze qui s'immole / ghast | Retraite (s'éloigner), repositionnement ; pas de combat à distance tant que non implémenté. |
| Perte de path (bloqué, `stuck`) | Re-path avec budget d'expansions augmenté, event `PathReset`. |
| Chute dans la lave | Considérée comme une mort probable → respawn + checkpoint. |
| Timeout I/O > 30 s sur une lecture réseau | Event `IoTimeout`, on continue (le keep-alive maintient la connexion). `read_packet` est cancel-safe. |
| Timeout global d'une phase (20 min) | Event `PhaseTimeout`, `Retry` (max 3), puis `Abort` propre. |

## 3. Règles de pathfinding prudent (`src/path.rs`)

- **Interdit** : parkour au-dessus de la lave dans le Nether (cellule liquide non
  traversable + aucun saut au-dessus d'un liquide).
- **Pénalités de coût** : eau (modérée — le bot ne nage pas), lave (exclue, forte
  pénalité d'adjacence), bords de falaise (chute > 3 blocs non générée ; pénalité sur
  les cellules en bordure de vide).

## 4. Garde-fous de mesure

- Aucun `unwrap`/`expect` sur donnée réseau ou chunk non chargé (hors `#[cfg(test)]`).
- Pas de téléport de debug pendant une mesure.
- Runs **offline-mode uniquement** (serveur local 1.21.1 cracked), jamais de serveur public.
- Validation à chaque itération : `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`,
  puis runs sur serveur local : **mêmes seeds** pour mesurer une phase, **seeds aléatoires**
  pour valider un run complet (≥ 3 runs).
- **Une seule phase par itération.** Toute affirmation de gain vient des splits réels.
