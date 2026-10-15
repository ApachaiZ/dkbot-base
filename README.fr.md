<div align="center">

# 🎛️ DakkaBot — dkbot-base

### Contrôle fin du VPS depuis Discord — un seul binaire Rust natif, zéro dépendance à l'exécution 🦀

[![Rust](https://img.shields.io/badge/Rust-%E2%89%A5%201.85-orange?logo=rust)](https://rust-lang.org)
[![serenity](https://img.shields.io/badge/serenity-0.12-5865F2?logo=discord)](https://crates.io/crates/serenity)
[![poise](https://img.shields.io/badge/poise-0.6-5865F2)](https://crates.io/crates/poise)
[![Licence](https://img.shields.io/badge/licence-MIT-blue)](#-licence)
[![CI](https://img.shields.io/badge/CI-fmt%20%2B%20clippy%20%2B%20tests-green)](https://github.com/ApachaiZ/dkbot-base/actions)

**Statut réel des machines** · actions power sécurisées · watchdog · sauvegardes ·
planificateur · hôte de plugins · i18n en/fr · **slash commands dynamiques depuis les manifestes des plugins**

> 🌐 Langues : 🇬🇧 [English](README.md) · 🇫🇷 Français · 🇩🇪 [Deutsch](README.de.md) · 🇪🇸 [Español](README.es.md)

</div>

---

## 📖 Sommaire

1. [🚀 TL;DR — prêt en 30 secondes](#-tldr--prêt-en-30-secondes)
2. [🎯 Ce que c'est](#-ce-que-cest)
3. [🏗️ Architecture (infographie)](#️-architecture-infographie)
4. [🔨 Construire, installer, mettre à jour](#-construire-installer-mettre-à-jour)
5. [⚙️ Configuration](#️-configuration)
6. [🎮 Commandes du cœur](#-commandes-du-cœur)
7. [🧩 Plugins — slash commands dynamiques](#-plugins--slash-commands-dynamiques)
8. [💾 Sauvegardes du jeu](#-sauvegardes-du-jeu)
9. [🔐 Sécurité](#-sécurité)
10. [🧪 Tests](#-tests)
11. [📄 Licence](#-licence)

---

## 🚀 TL;DR — prêt en 30 secondes

```bash
# 1. Seul prérequis : Rust ≥ 1.85
cargo build --release

# 2. Premier lancement : le setup interactif écrit ~/.config/dakkabot/.config.d/.env
./target/release/dkbot

# 3. En production (systemd) : jamais de prompt
./target/release/dkbot --non-interactive
```

> ⚠️ **Prérequis portail développeur** : activez l'intent privilégié
> **Server Members Intent** (portail → Bot → Privileged Gateway Intents).
> Sans lui, la passerelle Discord refuse la connexion.

---

## 🎯 Ce que c'est

**dkbot-base** est le cœur de DakkaBot : un bot Discord qui pilote une ou
plusieurs machines Linux (le VPS qui héberge votre serveur de jeu, ou des
hôtes distants en SSH) — plus un **hôte de plugins** : installez un plugin
(`.so`, ABI v1) et ses commandes apparaissent dans Discord comme de vraies
slash commands, sans recompiler le cœur.

## 🏗️ Architecture (infographie)

```text
        ┌─────────────────────────────────────────────────────────┐
        │                       DISCORD                            │
        │   /status /start /logs …   /dw … (plugin)   /saves …    │
        └───────────────┬─────────────────────────────────────────┘
                        │ gateway
        ┌───────────────▼─────────────────────────────────────────┐
        │                    dkbot (dkbot-base)                    │
        │  ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄  │
        │  █ power · status · logs · users · schedule · backups █  │
        │  █ watchdog · i18n en/fr · audit · confirmations      █  │
        │  █ hôte de plugins (ABI v1) · slash commands manifestes█ │
        │  ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀  │
        │       │ plugins/*.so ─── ABI v1 ───► dkbot-dragonwilds   │
        │       │                              dkbots-saves        │
        └───────┼──────────────────────────────────────────────────┘
                │ SSH / systemctl
        ┌───────▼──────────────┐   ┌───────────────────────────────┐
        │  VPS (local ou distant)│◄─│  Serveur de jeu (mod LD_PRELOAD)│
        │  services, disque…    │   │  API REST sur 127.0.0.1:8080   │
        └──────────────────────┘   └───────────────────────────────┘
```

- **Les plugins sont découplés** : chacun vit dans son propre dépôt,
  s'installe en `.so` précompilé (SHA-256 vérifié) et déclare ses commandes
  dans un **manifeste JSON** — l'hôte transforme ce manifeste en vraies
  slash commands Discord au démarrage.
- **Politique ABI** : un bump majeur d'`ABI_VERSION` = plugins à recompiler ;
  un plugin incompatible est refusé au chargement, jamais appelé.

---

## 🔨 Construire, installer, mettre à jour

```bash
cargo build --release          # → target/release/dkbot
dkbot plugin install <id>      # installer un plugin (SHA-256 vérifié)
dkbot plugin list              # plugins chargés
dkbot plugin remove <id>       # désinstaller
dkbot update                   # mise à jour auto depuis les releases GitHub (musl statique)
```

- Le premier lancement écrit `~/.config/dakkabot/.config.d/.env` (interactif).
- `--non-interactive` pour systemd — aucun prompt, jamais.
- Des binaires statiques précompilés (x86_64 + aarch64 musl) sont publiés à
  chaque tag `v*` par le workflow de release.

---

## ⚙️ Configuration

`~/.config/dakkabot/.config.d/.env` est lu en priorité, puis l'environnement.

| Variable | Rôle |
|---|---|
| `DISCORD_TOKEN` | Token du bot (obligatoire) |
| `DISCORD_CLIENT_ID` | Identifiant d'application (obligatoire pour les slash commands) |
| `DISCORD_OWNER_ID` | Identifiant du propriétaire — accès total |
| `DISCORD_GUILD_IDS` | Guildes autorisées (séparées par des virgules) |
| `DISCORD_ALERT_CHANNEL_ID` | Canal des alertes du watchdog |
| `DISCORD_AUDIT_CHANNEL_ID` | Canal d'audit (actions confirmées, sauvegardes…) |
| `ALERT_INTERVAL_MS` | Intervalle d'alerte du watchdog |
| `TARGETS` / `TARGET_HOST` | Machines cibles `alias:hôte` (la première est la cible par défaut) |
| `SERVICES` / `SERVICE` | Unités systemd `alias:unité` (la première est le défaut) |
| `SSH_USER` `SSH_PASSWORD` `SSH_KEY_PATH` | Accès SSH aux cibles |
| `SSH_KNOWN_HOSTS_FILE` `SSH_TIMEOUT_SECS` | Durcissement SSH |
| `SYSTEMCTL_PREFIX` | ex. `sudo -n` quand root est requis |
| `LOG_FILE` `LOG_LINES` | Stockage des logs / nombre de lignes par défaut |
| `SAVES_PATH` `SAVES_COPY_TIMEOUT_SECS` | Copie des sauvegardes du jeu (voir 💾) |
| `WATCHDOG_CPU_PCT` `WATCHDOG_RAM_PCT` `WATCHDOG_DISK_PCT` | Seuils du watchdog |
| `BOT_LANGUAGE` | `en` / `fr` |

Les variables spécifiques aux plugins (`RSDW_API_TOKEN`, `DKBOT_SAVES_PATH`…)
vivent dans le même fichier et sont documentées dans le README de chaque plugin.

---

## 🎮 Commandes du cœur

| Commande | Rôle |
|---|---|
| `/status` | Statut réel de la machine : CPU, RAM, disque, services |
| `/start` `/stop` `/restart` `/reboot` | Actions power **avec confirmation** |
| `/ping` | Latence de la passerelle |
| `/logs [n]` `/botlogs [n]` | Logs du jeu / logs du bot (tail) |
| `/users` | Utilisateurs système (sessions, dernière connexion) |
| `/schedule` | Tâches récurrentes (timers systemd) |
| `/backups` | Archives des sauvegardes du jeu (voir 💾) |
| `/exec` | Exécution de commandes sur liste blanche |
| `/target` `/server` | Sélection de la machine / du service |
| `/plugin` | Gestion des plugins (install / list / remove / run) |
| `/help` | Vue d'ensemble des commandes |

Toutes les actions sensibles sont conditionnées aux rôles owner/opérateur et
exigent une confirmation explicite dans Discord.

---

## 🧩 Plugins — slash commands dynamiques

Un plugin est un `cdylib` qui dépend du crate `dkbot-host-api` (ABI v1),
définit sa vtable statique et appelle `export_plugin!(&VTABLE)` — voir
`plugins/dummy-plugin/` pour l'exemple minimal, et les vrais plugins :

- 🐉 **[dkbot-dragonwilds](https://github.com/ApachaiZ/dkbot-dragonwilds)** — 19 commandes pour administrer un serveur RuneScape: Dragonwilds (`/dw …`).
- 💾 **[dkbots-saves](https://github.com/ApachaiZ/dkbots-saves)** — archives des sauvegardes du monde (`/saves …`).

**Du manifeste à Discord, en direct :**

```json
{"group": "dw", "description": "Administration du serveur Dragonwilds", "commands": [
  {"name": "tp", "description": "Téléporter un joueur sur un autre",
   "args": [{"name": "from", "kind": "player", "required": true},
            {"name": "to",   "kind": "player", "required": true}]}
]}
```

L'hôte transforme chaque commande du manifeste en sous-commande typée au démarrage :

- `kind: player|item` → **autocomplétion** (le plugin répond à un appel `__ac`
  avec jusqu'à 25 suggestions — pseudos, noms d'objets) ;
- `choices: ["on","off"]` → menu de sélection fixe ;
- `permissions: "admin"` → groupe séparé `{groupe}-admin` visible uniquement
  des administrateurs de la guilde (le propriétaire le voit toujours) ;
- un résultat de plugin au format `{"card": {title, description, tone, fields}}`
  est rendu en carte Discord standard — tout le reste retombe sur un bloc
  JSON (les anciens plugins restent compatibles) ;
- si un manifeste change en cours d'exécution (ex. le switch cheat de
  dragonwilds), l'hôte ré-enregistre les commandes de la guilde
  automatiquement — sans redémarrage.

---

## 💾 Sauvegardes du jeu

`/backups` archive le dossier des sauvegardes du jeu (configurable via
`SAVES_PATH`), conservées et purgées localement. Le plugin compagnon
**dkbots-saves** ajoute `/saves backup|list|restore` (propriétaire
uniquement) avec emplacement d'archive et rétention configurables.

---

## 🔐 Sécurité

- 🔐 Token du bot et secrets dans `.env` — jamais dans le dépôt.
- ✅ **Prompts de confirmation** pour chaque action destructrice.
- 🛡️ Contrôle des rôles owner / opérateur sur toutes les commandes sensibles.
- 📝 Chaque action confirmée est auditée dans le canal d'audit et les logs.
- 🚫 Les plugins ne se chargent que depuis `~/.config/dakkabot/plugins/`,
  avec vérification SHA-256 à l'installation et contrôle d'ABI versionné au chargement.

---

## 🧪 Tests

```bash
cargo test                          # 84 tests unitaires — aucun réseau, aucun fichier hors tmp/
DKBOT_TEST_PLUGINS=/tmp/plugins-test cargo test -- --ignored
                                    # intégration : charge de VRAIS plugins .so, vérifie
                                    # manifestes, flags d'autocomplétion, règles d'ordre
```

---

## 📄 Licence

MIT.
