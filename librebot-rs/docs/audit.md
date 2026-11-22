# Audit complet de librebot-rs

Audit réalisé par lecture intégrale du code source (branche `main`, commit `0c5b386`),
complété par l'exécution de `cargo test`. Le wiki des compétences est dans
[`wiki.md`](wiki.md).

## 1. Périmètre

| Élément | Valeur |
|---|---|
| Code Rust (hors généré) | ~2 600 lignes (`src/`) + 116 lignes binaire |
| Code généré | `blocks_gen.rs` (1 119 L, 1 060 blocs), `items_gen.rs` (1 355 L, 1 333 items) |
| Tests | 11 tests unitaires (dans les modules) + 6 tests d'intégration (serveur mock loopback) |
| Dépendances | 3 : `tokio` 1.x (full), `flate2` 1.x, `md-5` 0.10 — aucune dépendance Minecraft/Node |
| Protocole cible | 767 (Minecraft Java 1.21.1), mode offline (cracked) |
| Édition | Rust 2021 |

Fichiers : `src/protocol.rs`, `src/nbt.rs`, `src/world.rs`, `src/path.rs`,
`src/inventory.rs`, `src/recipes.rs`, `src/mission.rs`, `src/bot.rs`, `src/config.rs`,
`src/bin/librebot.rs`, `src/lib.rs`, `examples/ping.rs`, `gen/blocks.py`, `gen/items.py`,
`tests/{full_session,chat,inventory,mine,walk_jump,world_physics}.rs` + `tests/common/`.

## 2. Architecture

```
bin/librebot.rs ──► boucle reconnexion + session (stdin, tick 50 ms, mission auto)
        │
        ▼
   bot.rs ──► Bot : connect() (handshake→login→compression→configuration)
        │     play_turn() : boucle de lecture des paquets play (événements)
        │     tick(dt) : physique + suivi de chemin + minage en cours
        │     actions : goto / dig / place / say / select_hotbar / click_slot
        ▼
   protocol.rs ──► Conn : trames VarInt + zlib, read_packet cancel-safe
   nbt.rs      ──► parseur NBT (tags 0-12) + texte des composants chat
   world.rs    ──► chunks paletted (24 sections), block_state / is_solid / set_block_state
   path.rs     ──► A* 4 directions, montées 1 bloc, chutes ≤ 3, budget 20 000 expansions
   inventory.rs──► slots 767, window_items / set_slot, clics de fenêtre
   recipes.rs  ──► 35 entrées de recettes (43 items) + item_id par nom
   mission.rs  ──► mission survie : phase_wood / phase_stone / phase_iron
   blocks_gen / items_gen ──► tables générées depuis minecraft-data (gen/*.py)
```

## 3. Couverture protocolaire (767 / 1.21.1)

### 3.1 Phases gérées
- **Handshake** + **Login** : `login_start` (nom + UUID offline 16 octets), compression
  (Set Compression), `login_success`, `login_acknowledged`, refus avec raison lue.
- **Configuration** : keep-alive, ping/pong, `select_known_packs`
  (`minecraft:core:1.21.1`), `client_information` (fr_FR, view distance 8, main droite,
  skin complète, filtres off), `finish`.
- **Jeu (play)** : voir tableau ci-dessous.

### 3.2 Paquets clientbound traités (`play_turn`, `src/bot.rs:270`)

| ID 767 | Paquet | Action |
|---|---|---|
| 0x26 | keep_alive | réponse immédiate |
| 0x2b | login (jeu) | entity id, dimension, gamemode → `Event::Connected` |
| 0x0d | chunk_batch_start | reset compteur de batch |
| 0x0c | chunk_batch_finished | `chunk_batch_received` (64 chunks/tick demandés) |
| 0x27 | map_chunk | décodage 24 sections paletted (blocs + biomes) |
| 0x21 | unload_chunk | retrait du chunk |
| 0x09 | block_change | mise à jour locale d'un bloc |
| 0x13 | window_items | inventaire (fenêtre 0 ou conteneur) |
| 0x15 | set_slot | mise à jour d'un slot |
| 0x53 | held_item_slot | ignoré (le bot choisit) |
| 0x40 | position | téléport : flags relatifs gérés + `teleport_confirm` → `Event::Spawned` |
| 0x6c | system_chat | NBT → texte → `Event::Chat` |
| 0x39 | player_chat | décodage best effort → `<pseudo> message` |
| 0x1d | kick_disconnect | raison lue → `Event::Disconnected` |
| 0x77 | declare_recipes | noms de recettes stockés (types 0,1,2-23 skippés proprement) |
| 0x33 | open_window | mémorise le windowId (table de craft) |
| 0x12 | close_window | ferme la fenêtre |

### 3.3 Paquets serverbound envoyés

Handshake 0x00, login_start 0x00, login_acknowledged ; configuration : settings 0x00,
finish 0x03, keep_alive 0x04, pong 0x05, select_known_packs 0x07 ; play :
teleport_confirm 0x00, chat_message 0x06 (format 767 : timestamp, salt, signature absente,
ack 3 octets), chunk_batch_received 0x08, window_click 0x0e (mode 0 pick/place),
container_close 0x0f, keep_alive 0x18, position 0x1a, position_look 0x1b,
craft_recipe 0x22 (makeAll), block_dig 0x24 (start/finish, séquence), held_item_slot 0x2f,
swing 0x36, block_place 0x38, use_item 0x39.

### 3.4 Non géré (volontairement)

Entités et combat (`use_entity` déclaré mais jamais envoyé), dégâts/soin (set_health,
experience déclarés, ignorés), particules, sons, titres, boss bars, advancements,
statistiques, blocs multi-changement 0x49 (déclaré, ignoré), inventaires de conteneurs
type coffre/fourneau (fenêtre ouverte lue, mais aucune interaction), joueur fantôme
(game event, respawn ignorés), éclairage (masques lumière ignorés à la lecture),
chat signé avancé (signatures ignorées), commandes slash (envoi chat brut uniquement).

## 4. Modules — points notables

- **`protocol.rs`** : `Conn::read_packet` est *cancel-safe* (tampon interne, utilisé dans
  `tokio::select!` sans désynchronisation). Trame max 8 Mo, longueur 0 ou > max → erreur.
  Compression zlib seuil serveur. `Body` : tous les lecteurs sont bornés (aucun panic sur
  paquet tronqué). UUID offline = MD5 v3 façon vanilla (`OfflinePlayer:<nom>`).
- **`nbt.rs`** : tags 0-12, listes/composés bornés (1024/4096 éléments max). Extraction de
  texte : `text`, `extra`, `translate`.
- **`world.rs`** : heightmaps = NBT anonyme (767) ; 24 sections de 4096 blocs + 64 biomes,
  palettes : valeur unique (0 bit), palette ≤ 8 bits, stockage direct. `set_block_state`
  reconstruit la section en direct 15 bits. Monde : y ∈ [-64, 320).
- **`path.rs`** : A* 4 directions, montée 1 bloc (coût 1,6), chutes ≤ 3 blocs, coût
  heuristique Manhattan + 1,1·Δy, budget 20 000 expansions.
- **`bot.rs` `tick`** : suivi des waypoints (vitesse 4,3 m/s), saut (8,95 m/s) si blocage,
  gravité 32 m/s², vitesse terminale −78,4 m/s, détection d'atterrissage sur le sommet du
  bloc. Minage : durée estimée depuis la dureté du bloc (mains nues), envoi `block_dig`
  finish quand le timer expire, bloc marqué air localement.
- **`inventory.rs`** : layout fenêtre 0 (0 résultat, 1-4 grille 2x2, 5-8 armure, 9-35
  stockage, 36-44 hotbar, 45 offhand) ; mapping conteneur → slots joueur ; composants
  d'items skippés pour un sous-ensemble de types (slot illisible sinon, inventaire partiel).
- **`mission.rs`** : recherche de blocs dans les chunks chargés, approche + minage +
  ramassage (goto sur le bloc miné), craft via recette déclarée par le serveur, pose et
  ouverture de la table de craft, sélection d'item en hotbar (avec déplacements par clics).
- **`config.rs`** : parsing JSON naïf de premier niveau (sans serde), surcharges CLI.

## 5. Tests et vérification

- `cargo test` : **17 tests, 0 échec** (11 unitaires + 6 intégration).
- Intégration sur **serveur mock 1.21.1 complet en loopback** (`tests/common/mod.rs` :
  handshake → login → compression → configuration → play) : session complète, chat
  système + joueur, inventaire (window_items/set_slot/clic), minage + pose, marche + saut
  + arrivée, chute + atterrissage.
- Aucun test ne requiert de serveur réel ni de réseau externe.

## 6. Robustesse et sécurité

Points forts :
- Tous les parseurs (`Body`, NBT, palettes, slots, recettes, chat) sont bornés — pas de
  panic sur entrée malformée ; les erreurs de décodage dégradent gracieusement
  (`eprintln!`, état partiel).
- Trames plafonnées (8 Mo), `drain_packets` borné (300 paquets / 50 ms), reconnexion
  automatique avec délai configurable.
- Lecture cancel-safe, donc utilisable dans `tokio::select!`.
- Mode offline : **aucun token, aucune donnée d'authentification** ; connexion en clair
  (TCP sans TLS, normal pour Minecraft vanilla).
- Debug par variable d'environnement `LIBREBOT_DEBUG` (dump des IDs de paquets).

Points de vigilance :
- `recipes::item_id()` **panique** sur un nom d'item inconnu (utilisé avec des noms en
  dur dans `mission.rs` — acceptable en l'état, à remplacer par un `Option` pour du code
  générique).
- Quelques envois ignorent l'erreur (`send(...).await.ok()?`) : une perte de connexion en
  cours d'action est détectée au prochain `play_turn`/`tick`.
- La durée de minage est une estimation locale (dureté, mains nues) : pas de gestion du
  paquet `block_dig` annulé par le serveur (pas de "abort" envoyé si on change de cible).
- Pas de gestion de la limitation de débit (anti-cheat) côté serveur : le bot peut être
  kické sur des serveurs protégés.

## 7. Limites connues (confirmées au code)

1. Protocole **767 uniquement** (1.21.1) ; les constantes 775 existent en commentaire mais
   `PROTOCOL_VERSION = 767`.
2. Pas de nage/échelles, pas de diagonales dans le pathfinding, pas de traversée d'eau.
3. Pas de combat (aucun envoi `use_entity`), pas de gestion de la faim/dégâts.
4. Slots avec composants d'item complexes non supportés (ignorés à la lecture).
5. Craft : uniquement les recettes de la table interne `recipes.rs`, via `craft_recipe`
  (le serveur doit avoir déclaré la recette) ; pas de fourneau (fondage non implémenté —
   la phase fer crafte directement si des lingots sont présents).
6. La pose suppose un bloc en main ; `place` ne vérifie pas le côté serveur.
7. Chat des joueurs en best effort (mentions, signatures, masquage non gérés).
8. Table des blocs : solidité issue des shapes 1.21.3 (noms stables), duretés 1.21.1.

## 8. Pistes d'amélioration (hors périmètre actuel)

- Rendre `item_id` non panissant ; configurer la version du protocole (775) proprement.
- Gérer `multi_block_change`, les entités (positions) et le combat de base.
- Interactions de conteneurs (coffre, fourneau : fondage du fer, cuisson).
- Anti-blocage : envoi `block_dig` abort si la cible change, attente de confirmation
  serveur (`block_change`) avant de marquer un bloc comme miné.
- Persistance de la carte du monde entre les sessions.
