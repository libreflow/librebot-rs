# Wiki des compétences de librebot-rs

Ce que le bot **sait faire aujourd'hui** (branche `main`, protocole 767 / Minecraft 1.21.1,
mode offline). Pour le détail technique et les limites : voir [`audit.md`](audit.md).

## 🧭 Navigation dans ce wiki

- [Connexion & session](#-connexion--session)
- [Perception du monde](#-perception-du-monde)
- [Déplacement & physique](#-déplacement--physique)
- [Minage & construction](#-minage--construction)
- [Inventaire & craft](#-inventaire--craft)
- [Chat & commandes](#-chat--commandes)
- [Mission survie autonome](#-mission-survie-autonome)
- [API publique (utilisable comme crate)](#-api-publique)
- [Recettes connues](#-recettes-connues)
- [Non supporté](#-non-supporté)

## 🔌 Connexion & session

| Compétence | Détail |
|---|---|
| Connexion TCP | handshake protocole 767, hôte/port personnalisables |
| Login offline | UUID dérivé MD5 v3 (`OfflinePlayer:<nom>`), zéro token — serveurs **Cracked: ON** |
| Compression | négocie et applique le seuil zlib du serveur |
| Phase configuration | ping/pong, keep-alive, `select_known_packs` (`minecraft:core:1.21.1`), `client_information` (fr_FR, distance de vue 8, main droite), finish |
| Keep-alive en jeu | réponse automatique, sinon déconnexion détectée |
| Reconnexion auto | boucle infinie, délai `reconnectSecs` (30 s par défaut) |
| Kick | raison décodée et affichée |
| Debug | variable d'environnement `LIBREBOT_DEBUG` → dump des paquets reçus |

## 🌍 Perception du monde

- Décodage des **chunks 1.21.1** (24 sections paletted : blocs + biomes), stockage en mémoire.
- Requêtes : `world.block_state(x,y,z)`, `world.is_solid(...)`, `world.block_name(...)`,
  `world.has_chunk(cx,cz)`, `world.chunk_count()`.
- Suivi des mises à jour : `block_change`, `unload_chunk`.
- Tables embarquées : **1 060 blocs** (solidité, dureté, temps de minage) et **1 333 items**.
- Recherche de blocs par nom dans un rayon autour du bot (`find_block`, `find_block_any`).

## 🚶 Déplacement & physique

- **Pathfinding A\*** natif : 4 directions, montées d'un bloc, chutes jusqu'à 3 blocs,
  budget 20 000 expansions (`goto x y z` → nombre d'étapes).
- Physique simulée côté client : gravité 32 m/s², vitesse terminale −78,4 m/s,
  vitesse de marche 4,3 m/s, saut ≈ 1,25 bloc (8,95 m/s).
- Saut automatique devant un obstacle ; détection d'atterrissage propre.
- Téléport serveur : position appliquée (flags relatifs) + confirmation envoyée.

## ⛏️ Minage & construction

- **Miner** un bloc : `dig(x, y, z)` envoie `block_dig`, durée estimée selon la dureté,
  envoi du finish automatique, bloc marqué comme air localement.
- **Poser** le bloc en main : `place(x, y, z, face)` (`block_place`).
- Sélection de slot de hotbar : `select_hotbar(0-8)`.
- `goto_and_dig(x, y, z)` : approche + minage + ramassage du drop (mission).

## 🎒 Inventaire & craft

- Suivi complet des slots joueur (fenêtre 0) : `window_items`, `set_slot`, état
  transporté (`carried`), `state_id` pour les clics.
- Requêtes : `inventory.find(item_id)` (hotbar prioritaire), `count(item_id)`, `held(i)`.
- **Clics de fenêtre** : `click_slot` (mode 0, ramasser/poser un stack) — utilisé pour
  déplacer un item vers la hotbar (`select_hotbar_by_item`).
- **Craft** : `craft("nom")` vérifie les ingrédients, retrouve la recette déclarée par le
  serveur (`declare_recipes`) et envoie `craft_recipe` (makeAll) dans la fenêtre ouverte
  (inventaire 2x2 ou table 3x3).
- **Pose + ouverture d'une table de craft** : `place_and_open_crafting_table()`.

## 💬 Chat & commandes

- Envoi de messages chat (`say`) au format 767 (timestamp, salt, sans signature).
- Lecture du **chat système** (NBT) et du **chat des joueurs** (`player_chat`, best effort,
  affiché `<pseudo> message`).
- Depuis le stdin en cours de session :
  - `goto x y z` → le bot marche jusqu'au bloc (A\*),
  - toute autre ligne → envoyée dans le chat du serveur.
- CLI : `--host <hôte> --port <port> --username <nom>` ; `config.json` (`host`, `port`,
  `username`, `reconnectSecs`) à côté de l'exécutable.

## 🎯 Mission survie autonome

Lancée **automatiquement à la connexion** (`src/bin/librebot.rs`), enchaînement :

1. **Phase bois** (`phase_wood`) : attend les chunks (≥ 25, max 15 s), récolte 6 bûches
   (8 essences reconnues), craft planches → table de craft → pose et ouvre la table →
   craft stick, pioche, hache et épée en bois.
2. **Phase pierre** (`phase_stone`) : mine ≥ 12 cobblestone, craft pioche/hache/épée en
   pierre + fourneau.
3. **Phase fer** (`phase_iron`) : mine ≥ 5 minerais de fer ; craft équipement en fer si
   des lingots sont disponibles (**fondage au fourneau non implémenté**).

En cas d'échec d'une phase, le bot passe à la suite et reste en attente de commandes.

## 📚 API publique

Le crate `librebot_rs` expose : `bot::{Bot, Event, ids}`, `protocol::{Conn, Body,
ReadOutcome, PROTOCOL_VERSION, MC_VERSION, offline_uuid, write_varint, write_string,
write_position}`, `world::World`, `path::{find_path, walkable, MAX_EXPANSIONS}`,
`inventory::{Inventory, ItemStack, read_slot, window_click_body, HOTBAR_FIRST…}`,
`recipes::{recipe_for, item_id, item_exists, Recipe}`, `nbt::{parse, text}`,
`blocks_gen::{MC_BLOCKS, block_name, block_state_by_name, is_solid, hardness,
break_time_secs}`, `items_gen::{MC_ITEMS, item_name}`, `config::Config`.

Méthodes principales de `Bot` : `connect`, `play_turn`, `tick`, `drain_packets`,
`goto`, `path_len`, `dig`, `mining_done`, `place`, `select_hotbar`, `click_slot`, `say`,
`send_position`, `find_block`, `find_block_any`, `goto_and_dig`, `craft`,
`place_and_open_crafting_table`, `select_hotbar_by_item`, `count_item`, `dump_inventory`,
`phase_wood`, `phase_stone`, `phase_iron`.

## 🍳 Recettes connues

35 entrées couvrant 43 items (`src/recipes.rs`) : planches (9 essences), table de craft,
stick, outils bois/pierre/fer/diamant (pioche, hache, épée, pelle), armure fer et diamant
(casque, plastron, jambières, bottes), fourneau, bouclier, seau, briquet, torche, coffre,
bol, arc, flèche, **œil de l'Ender** (perle de l'Ender + poudre de blaze).

## 🚫 Non supporté

Nage/échelles, diagonales de déplacement, combat, dégâts/faim, conteneurs (coffre, fourneau
pour fondre/cuire), entités, particules/sons, autres versions du protocole que 1.21.1,
slots avec composants NBT complexes. Voir [`audit.md`](audit.md) §7.
