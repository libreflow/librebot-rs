# ender-agent

Agent Minecraft autonome en Rust — architecture hybride **GOAP + Behavior Trees + mémoire du monde + apprentissage par renforcement**, avec [Azalea](https://github.com/azalea-rs/azalea) comme client Minecraft (Java **1.21.1**, protocole 768).

L'IA décide des **objectifs** ; des modules spécialisés exécutent. Aucun LLM ne pilote les actions à chaque tick.

## Architecture

```
HIGH-LEVEL AI      ender-planner    GOAP (A* sur l'espace d'etats), goals, utilite, graphe de progression
COGNITIVE ENGINE   ender-behavior   Behavior Trees reactifs, blackboard, interruptions
PERCEPTION         ender-perception WorldState : blocs, entites, inventaire, sante (push events + pull snapshots)
WORLD MODEL        ender-world-model carte observee (horodatage + confiance), memoire episodique (SQLite), connaissances
SKILLS ENGINE      ender-skills     MoveTo, Gather, Craft, Explore, Flee... verification dans l'etat reel
LEARNING ENGINE    ender-learning   environnement RL, recompenses ponderees, Q-learning tabulaire, replay
EVALUATION         ender-evaluation metriques, journal de decisions, benchmarks
EXECUTION          ender-minecraft  adaptateurs : SimAdapter (offline) et AzaleaAdapter (serveur reel)
APP                apps/agent       boucle cognitive : Observe -> Understand -> Evaluate -> Plan -> Execute -> Verify -> Learn
```

## Crates

| Crate | Responsabilité |
| --- | --- |
| `ender-minecraft` | `GameEvent`, trait `MinecraftAdapter`, `SimAdapter`, `AzaleaAdapter` (feature `live`), `Client` |
| `ender-perception` | `Value`, `BlockPos`, `WorldState`, `Perception`, entités, inventaire |
| `ender-world-model` | `WorldMap` (confiance décroissante), `Memory` (RAM + SQLite), `Knowledge` (recettes, blocs) |
| `ender-planner` | `GoapPlanner` (A*), `Goal`, `ProgressionGraph` (petgraph), sélection par utilité |
| `ender-behavior` | `BehaviorTree`, `Selector`/`Sequence`/`Inverter`/`Check`/`Action`, `Blackboard` |
| `ender-skills` | trait `Skill` (`can_execute`/`start`/`tick`/`cancel`), `SkillLibrary`, 6 familles de compétences |
| `ender-learning` | `Environment`, `Reward`, `TabularPolicy` (Q-learning), `ReplayBuffer`, `Trainer` |
| `ender-evaluation` | `Metrics`, `DecisionLog` ([OBSERVATION]/[PLAN]/...), benchmarks navigation/planification/mémoire/adaptation |
| `ender-agent` | binaire : boucle cognitive complète |

## Version d'Azalea et toolchain

- `azalea = "=0.10.3+mc1.21.1"` est **épinglé** (tag `v0.10.3+mc1.21.1` du 2024-10-23, Minecraft **1.21.1**, protocole 768) : la branche principale d'Azalea suit les versions récentes (26.x) et casserait la compatibilité. Pour une autre cible 1.21.x, changer le tag (ex. `v0.11.0+mc1.21.4`).
- Azalea 0.10.x requiert une toolchain **nightly d'époque** : `rust-toolchain.toml` épingle `nightly-2024-11-01` (les nightly récentes cassent `simdnbt`/`azalea-core` via des features retirées).
- `serde` est épinglé à `=1.0.210` (serde ≥ 1.0.228 casse `azalea-chat` via `serde::__private`), et `Cargo.lock` est commité : il reprend le lockfile d'Azalea au tag pour verrouiller tout l'arbre de dépendances à des versions compatibles.

## Utilisation

```bash
cd ender-agent

cargo test                                            # tests unitaires + simulation offline (SimAdapter)

cargo run -p ender-agent                              # mode simulation (aucun serveur requis)
ENDER_MAX_SECS=60 cargo run -p ender-agent             # simulation bornée à 60 s

cargo run -p ender-agent --features live              # serveur reel (offline mode)
ENDER_MODE=live ENDER_SERVER=127.0.0.1:25565 ENDER_USERNAME=ender \
  cargo run -p ender-agent --features live
```

Variables d'environnement : `ENDER_MODE` (sim|live), `ENDER_SERVER`, `ENDER_USERNAME`, `ENDER_DB` (chemin SQLite, vide = mémoire RAM), `ENDER_LOG`, `ENDER_DECISION_MS`, `ENDER_TICK_MS`, `ENDER_SAMPLE_MS`, `ENDER_MAX_SECS`.

## Phases de développement

Ce socle couvre la **phase 1** (infrastructure, client, événements, premier moteur cognitif) et pose les interfaces des phases 2–10 : perception riche (phase 2), pathfinding Azalea avancé (phase 3), skills vérifiés (phase 4), GOAP complet (phase 5), BT avec interruptions (phase 6), mémoire persistante (phase 7), graphe de progression (phase 8), RL sur sous-problèmes (phase 9), benchmarks bout-en-bout (phase 10).

## Limites connues du socle

- `AzaleaAdapter::craft` n'est pas encore câblé (clics d'inventaire Azalea : phase 4) ; la simulation fonctionne de bout en bout.
- Les blocs côté live sont rapportés par id d'état de bloc (résolution par nom via le registre Azalea : phase 2).
- `candle` (phase 9) sera ajouté en dépendance optionnelle de `ender-learning` le moment venu.
