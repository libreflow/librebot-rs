# librebot-rs

Bot Minecraft Java Edition en **Rust pur** : client protocole natif, **sans Node, sans
mineflayer, zéro token**. Réécriture Rust de [librebot](https://github.com/libreflow/librebot).

> État actuel : socle protocolaire validé **et mission survie autonome** — à la connexion,
> le bot enchaîne automatiquement les phases bois → pierre → fer (récolte, craft, minage),
> puis attend tes commandes dans le chat.

## ✅ Ce qui fonctionne (validé par `cargo test`)

- Trames protocole + **compression zlib** (Set Compression des deux côtés)
- **Login offline** (mode cracké) avec UUID dérivé comme un client vanilla
  (`OfflinePlayer:<nom>`, MD5 v3)
- Phase **configuration** : ping/pong, keep-alive, `select_known_packs`, `finish`
- Phase **jeu** : keep-alive automatique, lecture du chat système **et des joueurs**
  (`player_chat`), **envoi de chat**
- **Lecture des chunks** : décodage des sections paletted (blocs + biomes),
  cartographie du monde interrogeable (`world.block_state`, `world.is_solid`)
- **Position + téléport** : réception du spawn, confirmation des téléports
- **Physique** : chute avec gravité (32 m/s², vitesse terminale −78,4 m/s),
  détection du sol et atterrissage propre sur le sommet du bloc
- **Minage / pose de blocs** : `Player Digging` + `Use Item On`, sélection de la hotbar
- **Inventaire** : suivi des slots (`Window Items`, `Set Slot`), recherche d'item,
  clics de fenêtre (`Click Window`) et **craft** (`Craft Recipe`, recettes 2x2 / 3x3)
- **Mission survie autonome** : récolte de bûches, planches, table de craft posée et
  ouverte, outils en bois → pierre → fer (le fondage au fourneau reste à faire)
- **Reconnexion auto** (30 s par défaut) et détection des kicks avec raison
- Chat depuis stdin : tape un message, il est envoyé au serveur

## 🎯 Compatibilité

- **Minecraft Java 1.21.1** (protocole 767)
- Serveurs vanilla / Paper / Aternos en mode **Cracked: ON**
- Fonctionne sur un monde solo ouvert au LAN

## 🚀 Démarrage

```bash
cargo build --release
cp config.example.json config.json
# édite host/port puis :
cargo run --release
```

Surcharges CLI : `--host xxx.aternos.me --port 25565 --username LibreBot`.

Une fois connecté, la **mission survie se lance toute seule** (phases bois, pierre, fer).
Ensuite le bot affiche le chat et envoie tout ce que tu tapes sur stdin.
Commandes : `goto x y z` pour marcher jusqu'à un bloc (A*), tout le reste est dit dans le chat.

## 🧪 Tests

```bash
cargo test
```

`tests/full_session.rs` fait tourner un **serveur mock 1.21.1 complet** sur loopback
(handshake → login → compression → configuration → jeu) et valide chaque étape du
protocole — aucun serveur réel nécessaire. Les autres tests couvrent le chat,
l'inventaire/craft, le minage/la pose, la marche/le saut et la physique du monde.

## 🏗️ Architecture

```
src/protocol.rs      VarInt, chaînes, trames, compression zlib, UUID offline
src/nbt.rs           NBT minimal + extraction de texte des composants de chat
src/path.rs          A* sur la grille des blocs (montées, chutes, budget d'expansions)
src/world.rs         Chunks paletted, requêtes block_state / is_solid
src/blocks_gen.rs    Table des 1060 blocs (générée, gen/blocks.py)
src/items_gen.rs     Table des items + IDs numériques (générée, gen/items.py)
src/inventory.rs     Slots 1.21, Window Items / Set Slot, clics de fenêtre
src/recipes.rs       Recettes de craft (2x2 / 3x3) et table des ingrédients
src/mission.rs       Mission survie autonome : phases bois / pierre / fer
src/bot.rs           Session : login→configuration→jeu + physique (chute, marche, saut)
src/config.rs        config.json + surcharges CLI
src/bin/librebot.rs  Point d'entrée : reconnexion auto, mission auto, chat stdin
```

Aucune dépendance à un client Minecraft externe : le protocole est parlé directement
sur TCP (tokio pour l'async, flate2 pour zlib).

## 🗺️ Feuille de route (le bot "survie")

- [x] Connexion fiable (keep-alive, reconnexion)
- [x] Lecture des chunks (blocs, biomes) et cartographie
- [x] Position du joueur + physique (gravité, contact au sol)
- [x] Saut et déplacement horizontal (4,3 m/s, saut 1,25 bloc)
- [x] Pathfinding A* natif (montées d'un bloc, chutes ≤ 3, 20k expansions)
- [ ] Nage / échelles
- [x] Minage / pose de blocs (Use Item On, Player Digging)
- [x] Inventaire / craft (Click Window, Craft Recipe)
- [x] Mission survie autonome (phases bois → pierre → fer)
- [ ] Fondage au fourneau (fer fondu, armure)
- [ ] Combat (armes, armures, esquive de creeper)
- [ ] Mission "tuer le dragon" (Nether, forteresse, End)

## ⚠️ Limites actuelles

- Slots avec composants d'items non supportés (items NBT customs ignorés) ; la pose
  suppose un bloc en main. Le déplacement ignore les diagonales et la nage.
- Chat des joueurs décodé en « best effort » (signatures/mentions non gérées).
- Protocole 1.21.1 uniquement (les autres versions exigeront une table d'IDs par version).
- La solidité des blocs vient des shapes 1.21.3 (noms stables) ; régénérer via
  `gen/blocks.py` si besoin.
