<div align="center">

# 🎛️ DakkaBot — dkbot-base

### Fine-grained VPS control from Discord — one native Rust binary, zero runtime dependencies 🦀

[![Rust](https://img.shields.io/badge/Rust-%E2%89%A5%201.85-orange?logo=rust)](https://rust-lang.org)
[![serenity](https://img.shields.io/badge/serenity-0.12-5865F2?logo=discord)](https://crates.io/crates/serenity)
[![poise](https://img.shields.io/badge/poise-0.6-5865F2)](https://crates.io/crates/poise)
[![License](https://img.shields.io/badge/licence-MIT-blue)](#-license)
[![CI](https://img.shields.io/badge/CI-fmt%20%2B%20clippy%20%2B%20tests-green)](https://github.com/ApachaiZ/dkbot-base/actions)

**Real machine status** · safe power actions · watchdog · backups · scheduler ·
plugin host · i18n en/fr · **dynamic slash commands from plugin manifests**

> 🌐 Languages: 🇫🇷 [Français](README.fr.md) · 🇩🇪 [Deutsch](README.de.md) · 🇪🇸 [Español](README.es.md)

</div>

---

## 📖 Contents

1. [🚀 TL;DR — up in 30 seconds](#-tldr--up-in-30-seconds)
2. [🎯 What it is](#-what-it-is)
3. [🏗️ Architecture (infographic)](#️-architecture-infographic)
4. [🔨 Build, install, update](#-build-install-update)
5. [⚙️ Configuration](#️-configuration)
6. [🎮 Core commands](#-core-commands)
7. [🧩 Plugins — dynamic slash commands](#-plugins--dynamic-slash-commands)
8. [💾 Game backups](#-game-backups)
9. [🔐 Security](#-security)
10. [🧪 Tests](#-tests)
11. [📄 License](#-license)

---

## 🚀 TL;DR — up in 30 seconds

```bash
# 1. Only prerequisite: Rust ≥ 1.85
cargo build --release

# 2. First run: interactive setup writes ~/.config/dakkabot/.config.d/.env
./target/release/dkbot

# 3. Production (systemd): never prompts
./target/release/dkbot --non-interactive
```

> ⚠️ **Developer portal prerequisite**: enable the privileged
> **Server Members Intent** (portal → Bot → Privileged Gateway Intents).
> Without it, the Discord gateway refuses the connection.

---

## 🎯 What it is

**dkbot-base** is the core of DakkaBot: a Discord bot that controls one or
several Linux machines (the same VPS that runs your game server, or remote
hosts over SSH) — plus a **plugin host**: install a plugin (`.so`, ABI v1)
and its commands appear in Discord as real slash commands, without
recompiling the core.

## 🏗️ Architecture (infographic)

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
        │  █ plugin host (ABI v1) · slash commands from manifests█ │
        │  ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀  │
        │       │ plugins/*.so ─── ABI v1 ───► dkbot-dragonwilds   │
        │       │                              dkbots-saves        │
        └───────┼──────────────────────────────────────────────────┘
                │ SSH / systemctl
        ┌───────▼──────────────┐   ┌───────────────────────────────┐
        │  VPS (local or remote)│◄──│  Game server (LD_PRELOAD mod) │
        │  services, disk, …    │   │  REST API on 127.0.0.1:8080   │
        └──────────────────────┘   └───────────────────────────────┘
```

- **Plugins are decoupled**: each lives in its own repo, is installed as a
  prebuilt `.so` (SHA-256 verified), and declares its commands in a **JSON
  manifest** — the host turns that manifest into real Discord slash commands
  at startup.
- **ABI policy**: a major `ABI_VERSION` bump means plugins must be rebuilt;
  an incompatible plugin is refused at load, never called.

---

## 🔨 Build, install, update

```bash
cargo build --release          # → target/release/dkbot
dkbot plugin install <id>      # install a plugin (SHA-256 verified)
dkbot plugin list              # loaded plugins
dkbot plugin remove <id>       # uninstall
dkbot update                   # self-update from GitHub releases (static musl)
```

- First run writes `~/.config/dakkabot/.config.d/.env` (interactive).
- `--non-interactive` for systemd — no prompts, ever.
- Prebuilt static binaries (x86_64 + aarch64 musl) are published on every
  `v*` tag by the release workflow.

---

## ⚙️ Configuration

`~/.config/dakkabot/.config.d/.env` is read first, then the environment.

| Variable | Role |
|---|---|
| `DISCORD_TOKEN` | Bot token (required) |
| `DISCORD_CLIENT_ID` | Application id (required for slash commands) |
| `DISCORD_OWNER_ID` | Owner user id — full access |
| `DISCORD_GUILD_IDS` | Authorised guilds (comma-separated) |
| `DISCORD_ALERT_CHANNEL_ID` | Watchdog alerts channel |
| `DISCORD_AUDIT_CHANNEL_ID` | Audit channel (confirmed actions, backups…) |
| `ALERT_INTERVAL_MS` | Watchdog alert interval |
| `TARGETS` / `TARGET_HOST` | Target machines `alias:host` (first is default) |
| `SERVICES` / `SERVICE` | systemd units `alias:unit` (first is default) |
| `SSH_USER` `SSH_PASSWORD` `SSH_KEY_PATH` | SSH access to targets |
| `SSH_KNOWN_HOSTS_FILE` `SSH_TIMEOUT_SECS` | SSH hardening |
| `SYSTEMCTL_PREFIX` | e.g. `sudo -n` when root is required |
| `LOG_FILE` `LOG_LINES` | Log storage / default line count |
| `SAVES_PATH` `SAVES_COPY_TIMEOUT_SECS` | Game saves copy (see 💾) |
| `WATCHDOG_CPU_PCT` `WATCHDOG_RAM_PCT` `WATCHDOG_DISK_PCT` | Watchdog thresholds |
| `BOT_LANGUAGE` | `en` / `fr` |

Plugin-specific variables (`RSDW_API_TOKEN`, `DKBOT_SAVES_PATH`, …) live in
the same file and are documented in each plugin's README.

---

## 🎮 Core commands

| Command | Role |
|---|---|
| `/status` | Real machine status: CPU, RAM, disk, services |
| `/start` `/stop` `/restart` `/reboot` | Power actions **with confirmation** |
| `/ping` | Gateway latency |
| `/logs [n]` `/botlogs [n]` | Game logs / bot logs (tail) |
| `/users` | System users (sessions, last login) |
| `/schedule` | Recurring tasks (systemd timers) |
| `/backups` | Game saves archives (see 💾) |
| `/exec` | Whitelisted command execution |
| `/target` `/server` | Target machine / service selection |
| `/plugin` | Plugin management (install / list / remove / run) |
| `/help` | Command overview |

All sensitive actions are gated by owner/operator roles and require explicit
confirmation in Discord.

---

## 🧩 Plugins — dynamic slash commands

A plugin is a `cdylib` that depends on the `dkbot-host-api` crate (ABI v1),
defines a static vtable and calls `export_plugin!(&VTABLE)` — see
`plugins/dummy-plugin/` for the minimal example, and the real plugins:

- 🐉 **[dkbot-dragonwilds](https://github.com/ApachaiZ/dkbot-dragonwilds)** — 19 commands to administer a RuneScape: Dragonwilds server (`/dw …`).
- 💾 **[dkbots-saves](https://github.com/ApachaiZ/dkbots-saves)** — world saves archives (`/saves …`).

**Manifest → Discord, live:**

```json
{"group": "dw", "description": "Dragonwilds server administration", "commands": [
  {"name": "tp", "description": "Teleport a player onto another",
   "args": [{"name": "from", "kind": "player", "required": true},
            {"name": "to",   "kind": "player", "required": true}]}
]}
```

The host turns every manifest command into a typed subcommand at startup:

- `kind: player|item` → **autocomplete** (the plugin answers a `__ac` call
  with up to 25 suggestions — nicknames, item names);
- `choices: ["on","off"]` → a fixed select menu;
- `permissions: "admin"` → a separate `{group}-admin` group visible only to
  guild administrators (the owner always sees it);
- a plugin result shaped `{"card": {title, description, tone, fields}}` is
  rendered as a standard Discord embed card — anything else falls back to a
  JSON block (older plugins keep working);
- if a manifest changes at runtime (e.g. the dragonwilds cheat switch),
  the host re-registers the guild commands automatically — no restart.

---

## 💾 Game backups

`/backups` archives the game saves directory (configurable via `SAVES_PATH`),
kept and pruned locally. The companion **dkbots-saves** plugin adds owner-only
`/saves backup|list|restore` with a configurable archive location and
retention.

---

## 🔐 Security

- 🔐 Bot token and secrets in `.env` — never in the repo.
- ✅ **Confirmation prompts** for every destructive action.
- 🛡️ Owner / operator role gating on all sensitive commands.
- 📝 Every confirmed action is audited in the audit channel and the logs.
- 🚫 Plugins are loaded from `~/.config/dakkabot/plugins/` only, with
  SHA-256 verification on install and a versioned ABI check at load.

---

## 🧪 Tests

```bash
cargo test                          # 84 unit tests — no network, no temp files outside tmp/
DKBOT_TEST_PLUGINS=/tmp/plugins-test cargo test -- --ignored
                                    # integration: loads REAL .so plugins, checks
                                    # manifests, autocomplete flags, ordering rules
```

---

## 📄 License

MIT.
