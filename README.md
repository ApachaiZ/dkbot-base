<div align="center">

# 🎛 DakkaBot — VPS Control Bot

### Contrôlez finement votre VPS depuis Discord — en binaire natif unique 🦀

[![Rust](https://img.shields.io/badge/Rust-%E2%89%A5%201.85-orange?logo=rust)](https://rust-lang.org)
[![serenity](https://img.shields.io/badge/serenity-0.12-5865F2?logo=discord)](https://crates.io/crates/serenity)
[![poise](https://img.shields.io/badge/poise-0.6-5865F2)](https://crates.io/crates/poise)
[![Licence](https://img.shields.io/badge/licence-MIT-blue)](#-licence)

**Statut réel des machines** · actions power sécurisées · watchdog · sauvegardes ·
planificateur · modules · i18n en/fr · un seul binaire, zéro dépendance système

</div>

---

## 📖 Sommaire

1. [🚀 TL;DR — démarrage en 30 secondes](#-tldr--démarrage-en-30-secondes)
2. [🎯 Qu'est-ce que c'est](#-quest-ce-que-cest)
3. [🏗️ Architecture (infographie)](#️-architecture-infographie)
4. [🔨 Construire, installer, mettre à jour](#-construire-installer-mettre-à-jour)
5. [⚙️ Configuration](#️-configuration)
6. [🎮 Les commandes Discord](#-les-commandes-discord)
7. [🧩 Modules](#-modules)
8. [💾 Sauvegardes du jeu](#-sauvegardes-du-jeu)
9. [🔐 Sécurité](#-sécurité)
10. [🚀 Déploiement](#-déploiement)
11. [🗂️ Structure du crate](#️-structure-du-crate)
12. [🧪 Tests](#-tests)
13. [🩺 Dépannage & limitations](#-dépannage--limitations-connues)
14. [📄 Licence](#-licence)

---

## 🚀 TL;DR — démarrage en 30 secondes

```bash
# 1. Prérequis unique : Rust ≥ 1.85 (rustup)
cargo build --release

# 2. Premier lancement : le setup interactif écrit ~/.config/dakkabot/.config.d/.env
#    (ou copiez le modèle : cp .config.d/.env.example ~/.config/dakkabot/.config.d/.env)
./target/release/dkbot

# 3. En production (systemd) : jamais de prompt
./target/release/dkbot --non-interactive

# 4. Tests : 33 (aucun réseau, aucun fichier hors dossier temporaire)
cargo test
```

> ⚠️ **Prérequis portail développeur** : activez l'intent privilégié
> **Server Members Intent** (portail développeur → Bot → Privileged Gateway
> Intents). Sans lui, la passerelle Discord refuse la connexion.

---

## 🎯 Qu'est-ce que c'est

Un bot Discord **100 % privé** pour contrôler finement un **VPS** : la
machine où tourne le bot (backend `local`) ou une machine distante
(backend `ssh`, pur Rust, aucune dépendance système). Il agit au niveau du
SERVEUR — pas au niveau d'un panneau d'hébergeur.

- 📊 **`/status`** — statut RÉEL de la machine cible : CPU (usage mesuré,
  modèle, cœurs), RAM, disque, charge (1/5/15 min), uptime système, uptime
  du service, OS, noyau, adresse, type de connexion — chaque ligne
  n'apparaît que si la machine la renseigne ; bouton **Rafraîchir**
  (10 min) ;
- ▶️⏹️🔁 **`/start` `/stop` `/restart`** — actions APPLICATIVES : elles ne
  touchent que l'unité systemd configurée (ex. `nginx`), jamais la
  machine. Double confirmation, verrou anti-collision, UNE seule commande
  sans retry, suivi silencieux à backoff 5 s → 30 s (2 états stables
  exigés, 10 min max, bouton d'annulation du suivi), détection du
  redémarrage par reset d'uptime du SERVICE, GIFs animés et estimation de
  durée ;
- 🔌 **`/reboot`** — redémarrage de la MACHINE elle-même, **réservé au
  propriétaire** (double confirmation + avertissement renforcé) ;
- 📜 **`/logs`** (owner) — dernières lignes de log du service (journald ou
  fichier `LOG_FILE`), avec filtre ;
- 💾 **sauvegardes** — à chaque stop/start, dès que l'arrêt est avéré,
  archive `.tar` de `SAVES_PATH` vers `<SAVES_PATH>-backups/` (horodatage
  UTC ; lock, anti-rafale 1 h, 20 conservées) ; `/backups list` les liste
  et `/backups restore` les restaure (owner) ;
- 🔁 **restart avec sauvegarde** — `/restart … with_backup` fait le cycle
  GARANTI stop → arrêt confirmé → sauvegarde → start ;
- 🔔 **watchdog** — alertes crash / CPU / RAM / disque sur TOUTES les
  cibles × services, machine injoignable / de retour, retour de service
  (cooldown 15 min, délai de grâce après arrêt volontaire) ;
- 🏓 **`/ping`** — latence aller-retour réelle vers la machine (SSH compris) ;
- 🎯 **`/target`** et **`/server`** — machine et service par défaut pour
  votre session ;
- 👑 **`/users add|remove|list|clear`** (owner) — opérateurs persistés
  **chiffrés AES-256-GCM** ;
- 🧩 **modules** — fonctionnalités optionnelles activées/désactivées À LA
  VOLÉE (`/modules enable|disable`) : watchdog, planificateur (`/schedule
  add daily 04:00 restart`), sauvegardes, diagnostics `/exec` (lecture
  seule, liste blanche, owner), métriques Prometheus textfile (zéro port) ;
- 🩺 **`dkbot doctor`** — diagnostic local de l'installation (config,
  cibles, services, sauvegardes, canaux) ;
- 👻 **100 % éphémère** + notification DM en fin d'action ;
- 🌐 **i18n en/fr** complète ;
- ⚡ enregistrement conditionnel des slash commands (hash + purge des
  doublons, une seule requête groupée).

Le tout tient dans **un seul binaire statique** : pas de runtime, pas de
dépendance système (TLS et SSH en pur Rust), démarrage en quelques
millisecondes, ~10 Mo de RSS en régime.

## 🏗️ Architecture (infographie)

```text
┌─────────────┐  slash commands + boutons   ┌─────────────────────────────────┐
│   Discord   │ ◄────────────────────────── │             dkbot               │
│  (ta guilde)│ ──────────────────────────► │  ┌───────────────────────────┐  │
└─────────────┘  cartes éphémères + DM      │  │    gateway poise/serenity  │  │
                                            │  └─────────────┬─────────────┘  │
   machines cibles                          │                │                │
┌──────────────────────┐                    │  ┌─────────────▼─────────────┐  │
│  local (le bot)      │◄── script POSIX ───│  │   backends : local | ssh   │  │
│  prod  (root@vps…)   │◄── SSH (russh) ────│  │   (TOFU known_hosts)      │  │
│  backup (game@10.…)  │◄── systemctl ─────►│  └─────────────┬─────────────┘  │
└──────────────────────┘                    │  ┌─────────────▼─────────────┐  │
                                            │  │ power : machine à états    │  │
   unités systemd (nginx, dragonwilds…)     │  │ verrou anti-collision,     │  │
   ◄── start / stop / restart / status ─────│  │ suivi 2 états stables      │  │
                                            │  └─────────────┬─────────────┘  │
                                            │  ┌─────────────▼─────────────┐  │
                                            │  │ modules : watchdog ·       │  │
                                            │  │ schedule · backups · exec ·│  │
                                            │  │ metrics · (dragonwilds*)   │  │
                                            │  └───────────────────────────┘  │
                                            └─────────────────────────────────┘
```

*Le module `dragonwilds` vit dans sa propre saveur de binaire — voir
[`ApachaiZ/DakkaBot-module-dragonwilds`](https://github.com/ApachaiZ/DakkaBot-module-dragonwilds).*

---

## 🔨 Construire, installer, mettre à jour

### 🚀 Installation rapide (cargo install)

```bash
# Depuis le dépôt cloné :
cargo install --path .
# Ou directement depuis GitHub (branche main) :
cargo install --git https://github.com/ApachaiZ/DakkaBot --branch main
```

Le binaire `dkbot` atterrit dans `~/.cargo/bin` (ajoutez ce dossier au
PATH). Les DONNÉES vivent dans **`~/.config/dakkabot/`** (config
`.config.d/`, logs, assets) — le premier lancement les crée via le setup
interactif. Pour tout mettre ailleurs (déploiement système) :

```bash
dkbot --root /opt/dakkabot run --non-interactive   # données dans /opt/dakkabot
```

Sans cargo, la sous-commande `install` copie DIRECTEMENT le binaire courant
(**SEUL le binaire est installé par défaut**, jamais de daemon sans le
demander) :

```bash
sudo dkbot install           # binaire dans /usr/bin et /bin (si réel),
                             # données dans ~/.config/dakkabot/
sudo dkbot install --daemon  # + unité systemd dakkabot.service (enable --now)
dkbot install /opt/dakkabot  # ou un dossier dédié : binaire + registre
                             # install.json, données au même endroit
```

`dkbot uninstall` retire le binaire (et l'unité systemd en root) en
CONSERVANT les données.

### 🔁 Mise à jour (`dkbot update`)

Les releases GitHub publient des binaires STATIQUES (musl, x86_64 et
aarch64) avec leurs sommes SHA-256 :

```bash
sudo dkbot update           # dernière release (root pour /usr/bin)
dkbot update v0.2.0         # version précise
```

L'asset est téléchargé dans un fichier temporaire, sa **somme SHA-256 est
vérifiée** (divergence = abandon), puis remplacé par `rename` atomique —
et le daemon supervisé, s'il tourne, est redémarré sur le nouveau binaire.
Les données ne sont jamais touchées.

---

## ⚙️ Configuration

`.config.d/.env` est lu en priorité, puis l'environnement (déploiement
systemd). Modèle complet : [`.config.d/.env.example`](.config.d/.env.example).
Si le fichier existe au lancement, le bot l'utilise tel quel — il ne pose
de questions que pour les valeurs requises manquantes.

| Variable | Rôle |
| --- | --- |
| `DISCORD_TOKEN`, `DISCORD_CLIENT_ID`, `DISCORD_OWNER_ID`, `DISCORD_GUILD_ID` | identité Discord (obligatoires) |
| `TARGETS` | `alias=spec,…` — `local` ou `[user@]hôte[:port]` ; la première cible est la cible par défaut |
| `SERVICES` | `alias=unité,…` (ex. `web=nginx`) — ou `SERVICE=myservice` en mono-service |
| `SSH_USER`, `SSH_KEY_PATH`, `SSH_PASSWORD`, `SSH_KNOWN_HOSTS_FILE`, `SSH_TIMEOUT_SECS` | connexion des cibles distantes |
| `SYSTEMCTL_PREFIX` | `sudo` / `sudo -n` si root est requis (localement ET à distance) |
| `DISCORD_ALERT_CHANNEL_ID`, `ALERT_INTERVAL_MS`, `WATCHDOG_*_PCT` | watchdog |
| `LOG_FILE`, `LOG_LINES` | source des `/logs` (défaut : journald) |
| `SAVES_PATH`, `SAVES_COPY_TIMEOUT_SECS` | copie des sauvegardes du jeu à chaque action power |

### Cibles multiples

```env
TARGETS=local,prod=root@vps.example.com,backup=game@10.0.0.5:2222
```

- `local` est un alias réservé : la machine du bot.
- Chaque cible a son propre backend ; `/target prod` bascule votre session.
- Les clés d'hôte SSH sont vérifiées en **TOFU fail-closed** : la première
  clé vue est enregistrée dans `SSH_KNOWN_HOSTS_FILE` puis RELUE pour
  vérification ; toute clé différente ensuite refuse la connexion
  (protection MITM) ; un fichier illisible ou non inscriptible refuse
  AUSSI la connexion (le TOFU ne se désactive jamais en silence). Sous une
  unité systemd durcie (`ProtectHome=true`), pointez
  `SSH_KNOWN_HOSTS_FILE` vers un chemin inscriptible.

### Droits systemd

`/start`, `/stop`, `/restart` et `/reboot` exécutent `systemctl` (avec le
préfixe configuré). Si le bot ne tourne pas en root :

- soit `SYSTEMCTL_PREFIX=sudo` avec une règle `sudoers` NOPASSWD ciblée :
  ```sudoers
  dakkabot ALL=(root) NOPASSWD: /usr/bin/systemctl start nginx, /usr/bin/systemctl stop nginx, /usr/bin/systemctl restart nginx, /usr/bin/systemctl reboot
  ```
- soit une politique polkit `org.freedesktop.systemd1.manage-units` pour
  l'utilisateur du bot (recommandé — aucun prompt, pas de sudo) :
  `/etc/polkit-1/rules.d/50-dakkabot.rules` :
  ```js
  polkit.addRule(function(action, subject) {
      if (action.id == "org.freedesktop.systemd1.manage-units" &&
          action.lookup("unit") == "nginx.service" &&
          subject.user == "apach") {
          return polkit.Result.YES;
      }
  });
  ```
  (`apach` = utilisateur qui lance le bot, à ajuster. Sans règle, `systemctl`
  retombe sur une authentification interactive — elle bloque le bot tant
  qu'un mot de passe n'est pas saisi, et échoue complètement sous systemd,
  sans TTY. `SYSTEMCTL_PREFIX` reste vide avec cette approche.)

## 🎮 Les commandes Discord

| Commande | Qui | Rôle |
|---|---|---|
| `/status` | opérateurs | dashboard réel de la cible (CPU/RAM/disque/load/uptime/OS/noyau/adresse) + Rafraîchir |
| `/start` `/stop` `/restart` | opérateurs | power sur l'unité systemd (confirmations, verrou, suivi, GIF, estimation) |
| `/restart … with_backup` | opérateurs | cycle stop → arrêt avéré → sauvegarde → start |
| `/reboot` | **owner** | reboot de la MACHINE (double confirmation renforcée) |
| `/ping` | opérateurs | latence réelle (SSH compris) |
| `/target` `/server` | par session | cible et service par défaut |
| `/logs [filtre]` | owner | dernières lignes du log du service |
| `/users add\|remove\|list\|clear` | owner | opérateurs (chiffrés) |
| `/backups list\|restore` | owner | sauvegardes du jeu |
| `/modules enable\|disable` | owner | activer/désactiver un module à la volée |
| `/schedule add\|remove\|list\|clear` | owner | planificateur (daily/weekly/once) |
| `/exec journal\|ss\|df\|ip\|uptime\|who` | owner | diagnostics lecture seule |

---

## 🔌 Plugins dynamiques (architecture modulaire)

Le cœur charge des **plugins** depuis `plugins/` (`.so`/`.dylib`/`.dll`) au
démarrage — sans recompilation :

```text
dkbot (dkbot-base)                 plugins/*.so (dépôts dédiés)
┌──────────────────────┐           ┌───────────────────────────────┐
│ plugin host (ABI v1) │◄── ABI ──►│ dragonwilds (dkbot-dragonwilds) │
│ /plugin (owner)      │   repr(C) │ saves      (dkbots-saves)      │
└──────────────────────┘  versionné└───────────────────────────────┘
```

- **Contrat** : le crate `dkbot-host-api` (dans ce dépôt, `crates/`) définit
  la vtable `repr(C)`, `ABI_VERSION` et les règles (rien que des octets à
  travers la frontière — les commandes échangent du JSON ; tampons bornés ;
  version incompatible = plugin refusé au chargement, jamais appelé).
- **Installation** :
  ```bash
  dkbot plugin install dragonwilds   # télécharge le .so + vérifie SHA-256
  dkbot plugin install saves
  dkbot plugin list | remove <id>
  ```
- **Exécution** : chaque plugin expose ses commandes JSON ; l'hôte les
  exécute via `/plugin <plugin> <commande> [args]` (owner), en transmettant
  le contexte appelant `{caller:{id,is_owner,roles}}` — les plugins font
  leur propre autorisation.
- **Écrire un plugin** : un crate `cdylib` qui dépend de `dkbot-host-api`,
  définit sa vtable statique et appelle `export_plugin!(&VTABLE)` (voir
  `plugins/dummy-plugin/` et les dépôts `dkbot-dragonwilds`/`dkbots-saves`).
- **Politique ABI** : bump majeur d'`ABI_VERSION` = plugins à recompiler
  (refus propre) ; ajouts compatibles (nouvelle commande) = pas de bump.

## 🧩 Modules

Les fonctionnalités optionnelles sont des MODULES : activables et
**désactivables à la volée** depuis Discord (`/modules enable|disable
<module>`, owner), persistés dans `modules.json` — aucun redémarrage.

| Module | Défaut | Rôle |
| --- | --- | --- |
| `watchdog` | activé | alertes crash / CPU / RAM / disque / cible injoignable, toutes cibles × services |
| `scheduler` | activé | `/schedule add|remove|list|clear` — actions récurrentes ou ponctuelles |
| `backups` | activé | `/backups list` et `/backups restore` |
| `exec` | **désactivé** | `/exec` — diagnostics en lecture seule (owner) |
| `metrics` | **désactivé** | export Prometheus textfile (`metrics.prom`) |

Chaque commande de module vérifie son état à l'entrée (carte « module
désactivé ») ; les boucles d'arrière-plan sautent leurs ticks — la
désactivation est immédiate.

### 📅 Planificateur

```text
/schedule add daily 04:00 restart            # chaque jour à 04:00
/schedule add weekly 2 04:00 restart         # chaque mardi (1=lundi…7=dimanche)
/schedule add once 2026-10-20 04:00 stop     # une seule fois (retirée après)
/schedule list | remove <id> | clear
```

L'exécution réutilise la mécanique power complète : verrou anti-collision,
UNE commande jamais répétée, suivi à 2 états stables, sauvegarde à l'arrêt
avéré — et le résultat part dans le canal d'audit
(`DISCORD_AUDIT_CHANNEL_ID`, repli : canal watchdog). Si le bot était
arrêté à l'heure dite, l'action court le jour même (rattrapage).

### 🔎 /exec (lecture seule)

`journal` (journalctl d'une unité), `ss`, `df`, `ip`, `uptime`, `who` —
owner uniquement, liste blanche FIXE, arguments validés côté Rust avant
d'atteindre le script embarqué (pas de shell, pas d'entrée libre).

### 🩺 dkbot doctor

```bash
dkbot doctor   # config, systemctl, pings des cibles, états des services,
               # accès aux sauvegardes, canaux, modules, planificateur
```

---

## 💾 Sauvegardes du jeu

`SAVES_PATH` (optionnel, demandé au setup interactif) pointe le dossier de
sauvegardes du jeu **sur la machine cible** (chemin absolu). À chaque
`/stop` et `/start`, dès que l'arrêt du service est **avéré**, le dossier
est archivé en `.tar` dans `<SAVES_PATH>-backups/` — le nom de l'archive
est l'**horodatage UTC** (`20261013-101501.tar`). La sûreté est mécanique :

- le script de copie vérifie lui-même que l'unité systemd est `inactive`/
  `failed` avant de copier, et **supprime** la copie si le service
  redémarre pendant celle-ci (snapshot incohérent) ;
- un **lock de sauvegarde** (mkdir atomique, périmé après 15 min) garantit
  qu'une seule copie s'exécute à la fois ;
- **anti-rafale** : pas de recopie si une copie réussie date de moins
  d'une heure ;
- **conservation** : les 20 sauvegardes les plus récentes seulement.

Sans `SAVES_PATH`, l'action power s'exécute normalement : la carte Discord
et les logs signalent que la sauvegarde n'est pas possible.
`SAVES_COPY_TIMEOUT_SECS` (défaut 600) borne une copie volumineuse.

---

## 🔐 Sécurité

- **Commandes construites sans shell** en local (arguments validés), et
  avec des éléments STRICTEMENT validés à distance (alias `[A-Za-z0-9_-]`,
  unités systemd `[A-Za-z0-9_.@:\-]`, préfixe filtré) — pas d'injection ;
- **UNE seule commande power par intention** : jamais de retry, l'incertitude
  est résolue par vérification d'état, pas par renvoi ;
- **Verrou anti-collision ATOMIQUE** : pose insert-si-absent sous mutex de
  processus + relecture de vérification (jeton unique) ;
- **Clés d'hôte SSH vérifiées** (TOFU fail-closed + refus en cas de
  changement ou d'impossibilité de mémoriser) ;
- **Opérateurs et sessions chiffrés au repos** (AES-256-GCM, clé dérivée du
  token Discord, fichiers 0600 posés AVANT le rename) — aucun repli en
  clair ;
- **Contrôle d'accès** : opérateurs pour status/power/ping, propriétaire seul
  pour `/reboot`, `/logs`, `/users`, `/modules`, `/schedule`, `/backups
  restore` et `/exec` ;
- **100 % éphémère**, mentions désactivées, guilde unique verrouillée.

---

## 🚀 Déploiement

Daemon **autonome** (superviseur embarqué) — pas de systemd par défaut.

```bash
# 1. Installer le binaire (cargo le pose dans ~/.cargo/bin) :
cargo install --git https://github.com/ApachaiZ/DakkaBot --branch main

# 2. Déploiement système : binaire dans /opt, données dans /opt/dakkabot.
sudo cp ~/.cargo/bin/dkbot /opt/dakkabot/   # voie cargo
# — ou —
sudo dkbot install /opt/dakkabot            # copie directe + registre install.json

# 3. Cycle de vie du daemon (superviseur embarqué) :
/opt/dakkabot/dkbot --root /opt/dakkabot start       # daemon détaché
/opt/dakkabot/dkbot --root /opt/dakkabot status      # état (pid, uptime)
/opt/dakkabot/dkbot --root /opt/dakkabot logs --lines 50
/opt/dakkabot/dkbot --root /opt/dakkabot restart     # redémarrage propre
```

Le superviseur relance le bot en cas de crash (backoff borné, **exit 78 =
abandon immédiat**, compteur remis à zéro après 30 s stables). Les données
vivent dans le dossier choisi : `.config.d/.env` (0600), `logs/`, `assets/`,
`daemon.json` (état du superviseur).

systemd reste possible avec l'unité de référence
[`deploy/dakkabot.service`](deploy/dakkabot.service) (durcie, `--root`
inclus), ou générée à la volée par `sudo dkbot install [<dir>] --daemon` :

```bash
sudo cp deploy/dakkabot.service /etc/systemd/system/dakkabot.service
sudo systemctl daemon-reload && sudo systemctl enable --now dakkabot
```

Le bot lui-même doit tourner sur la machine « local » si vous voulez la
piloter ; pour une cible distante, n'importe quelle machine suffit (SSH).

---

## 🗂️ Structure du crate

```
src/
├── main.rs           point d'entrée (setup → config → déploiement → gateway)
├── config.rs         chargement/validation du .env (exit 78)
├── targets.rs        cibles (local/SSH) + services (unités systemd), parsing validé
├── status.rs         état canonique d'une machine (vocabulaire commun)
├── daemon.rs         superviseur autonome + racine des données (--root, ~/.config/dakkabot)
├── backends/         les deux chemins d'exécution
│   ├── script.rs     script de collecte embarqué (POSIX) + parseur unique
│   ├── local.rs      exécution locale (tokio::process, timeout, classification)
│   ├── ssh.rs        SSH distant (russh, TOFU known_hosts, clé → mot de passe)
│   └── mod.rs        enum Backend { Local, Ssh } — même interface
├── api.rs            cache 5 s + déduplication des collectes en vol
├── power.rs          flux power complet + reboot (verrous, suivi, garanties)
├── commands.rs       slash commands + autocomplétions + handlers globaux
├── confirm.rs        double confirmation par boutons (60 s)
├── embeds.rs         cartes + dashboard modulaire
├── assets.rs         GIFs (disque → octets embarqués)
├── watchdog.rs       surveillance crash/CPU/RAM/disque/cibles (module)
├── modules.rs        framework de modules (/modules, modules.json)
├── schedule.rs       planificateur (/schedule, exécution headless)
├── backups.rs        /backups list|restore (scripts LIST/RESTORE)
├── exec.rs           /exec en lecture seule (liste blanche)
├── metrics.rs        export Prometheus textfile (module)
├── notify.rs         canal d'audit (DISCORD_AUDIT_CHANNEL_ID)
├── doctor.rs         dkbot doctor (diagnostic local)
├── logs_cmd.rs       /logs (journald ou fichier, filtre)
├── members.rs        roster des membres (cooldown opcode 8 persisté)
├── manage.rs         /users add|remove|list|clear
├── state.rs          opérateurs/sessions chiffrés, verrous persistants
├── crypto.rs         AES-256-GCM (clé dérivée du token Discord)
├── stats.rs          durées moyennes persistées (estimation ⏱)
├── deploy.rs         enregistrement des slash commands (hash + purge)
├── setup.rs          premier lancement interactif
└── i18n/             catalogues typés en/fr (parité = erreur de compilation)
```

---

## 🧪 Tests

```bash
cargo test   # parsing, crypto (roundtrip/altération), i18n, embeds, logs, daemon…
```

La suite ne touche ni réseau ni fichiers hors d'un dossier temporaire.
Le script de collecte embarqué est validable à la main :

```bash
SERVICE=nginx sh -s < <(extrait du script)   # sur une vraie machine
```

---

## 🩺 Dépannage & limitations connues

| Symptôme | Cause probable | Correctif |
|---|---|---|
| Connexion Discord refusée (`Disallowed gateway intents`) | Server Members Intent non activé | portail développeur → Bot → Privileged Gateway Intents |
| Le bot boucle au démarrage (exit 78) | `.env` invalide | réécrire la config ; exit 78 = abandon immédiat, c'est voulu |
| `systemctl` bloque en attente de mot de passe | pas de règle sudo/polkit | `SYSTEMCTL_PREFIX=sudo` (NOPASSWD) ou règle polkit |
| SSH refusé alors que rien n'a changé | clé d'hôte différente (TOFU) | vérifier `SSH_KNOWN_HOSTS_FILE` (MITM ?) ou le purger volontairement |
| Rotation du token Discord | fichiers d'état chiffrés illisibles | supprimer les fichiers d'état (documenté) |
| `/reboot` sur la cible locale | arrête le bot lui-même | unité systemd avec `Restart=on-failure` ou `enable` |
| Agent SSH | non supporté | clé privée ou mot de passe |

## 📄 Licence

MIT — Copyright (c) 2026 ApachaiZ.
