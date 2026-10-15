<div align="center">

# 🎛️ DakkaBot — dkbot-base

### Feine VPS-Steuerung aus Discord — ein einziges natives Rust-Binary, null Laufzeit-Abhängigkeiten 🦀

[![Rust](https://img.shields.io/badge/Rust-%E2%89%A5%201.85-orange?logo=rust)](https://rust-lang.org)
[![serenity](https://img.shields.io/badge/serenity-0.12-5865F2?logo=discord)](https://crates.io/crates/serenity)
[![poise](https://img.shields.io/badge/poise-0.6-5865F2)](https://crates.io/crates/poise)
[![Lizenz](https://img.shields.io/badge/lizenz-MIT-blue)](#-lizenz)
[![CI](https://img.shields.io/badge/CI-fmt%20%2B%20clippy%20%2B%20tests-green)](https://github.com/ApachaiZ/dkbot-base/actions)

**Echter Maschinenstatus** · sichere Power-Aktionen · Watchdog · Backups ·
Planer · Plugin-Host · i18n en/fr · **dynamische Slash Commands aus Plugin-Manifesten**

> 🌐 Sprachen: 🇬🇧 [English](README.md) · 🇫🇷 [Français](README.fr.md) · 🇩🇪 Deutsch · 🇪🇸 [Español](README.es.md)

</div>

---

## 📖 Inhalt

1. [🚀 TL;DR — in 30 Sekunden bereit](#-tldr--in-30-sekunden-bereit)
2. [🎯 Was es ist](#-was-es-ist)
3. [🏗️ Architektur (Infografik)](#️-architektur-infografik)
4. [🔨 Bauen, installieren, aktualisieren](#-bauen-installieren-aktualisieren)
5. [⚙️ Konfiguration](#️-konfiguration)
6. [🎮 Kern-Befehle](#-kernbefehle)
7. [🧩 Plugins — dynamische Slash Commands](#-plugins--dynamische-slash-commands)
8. [💾 Spiel-Backups](#-spielbackups)
9. [🔐 Sicherheit](#-sicherheit)
10. [🧪 Tests](#-tests)
11. [📄 Lizenz](#-lizenz)

---

## 🚀 TL;DR — in 30 Sekunden bereit

```bash
# 1. Einzige Voraussetzung: Rust ≥ 1.85
cargo build --release

# 2. Erster Start: interaktives Setup schreibt ~/.config/dakkabot/.config.d/.env
./target/release/dkbot

# 3. Produktion (systemd): niemals Prompts
./target/release/dkbot --non-interactive
```

> ⚠️ **Voraussetzung im Entwicklerportal**: aktiviert den privilegierten Intent
> **Server Members Intent** (Portal → Bot → Privileged Gateway Intents).
> Ohne ihn verweigert das Discord-Gateway die Verbindung.

---

## 🎯 Was es ist

**dkbot-base** ist der Kern von DakkaBot: ein Discord-Bot, der eine oder
mehrere Linux-Maschinen steuert (der VPS, auf dem dein Game-Server läuft,
oder entfernte Hosts per SSH) — plus ein **Plugin-Host**: Plugin installieren
(`.so`, ABI v1) und seine Befehle erscheinen in Discord als echte Slash
Commands, ohne den Kern neu zu kompilieren.

## 🏗️ Architektur (Infografik)

```text
        ┌─────────────────────────────────────────────────────────┐
        │                       DISCORD                            │
        │   /status /start /logs …   /dw … (Plugin)   /saves …    │
        └───────────────┬─────────────────────────────────────────┘
                        │ Gateway
        ┌───────────────▼─────────────────────────────────────────┐
        │                    dkbot (dkbot-base)                    │
        │  ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄  │
        │  █ power · status · logs · users · schedule · backups █  │
        │  █ watchdog · i18n en/fr · audit · bestätigungen      █  │
        │  █ plugin-host (ABI v1) · slash commands aus manifesten█ │
        │  ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀  │
        │       │ plugins/*.so ─── ABI v1 ───► dkbot-dragonwilds   │
        │       │                              dkbots-saves        │
        └───────┼──────────────────────────────────────────────────┘
                │ SSH / systemctl
        ┌───────▼──────────────┐   ┌───────────────────────────────┐
        │  VPS (lokal/entfernt) │◄──│  Game-Server (LD_PRELOAD-Mod) │
        │  Dienste, Disk…       │   │  REST-API auf 127.0.0.1:8080   │
        └──────────────────────┘   └───────────────────────────────┘
```

- **Plugins sind entkoppelt**: jedes lebt in seinem eigenen Repo, wird als
  vorkompiliertes `.so` installiert (SHA-256-geprüft) und deklariert seine
  Befehle in einem **JSON-Manifest** — der Host macht daraus beim Start echte
  Discord-Slash-Commands.
- **ABI-Politik**: ein Major-Bump von `ABI_VERSION` = Plugins müssen neu
  gebaut werden; ein inkompatibles Plugin wird beim Laden abgewiesen, nie
  aufgerufen.

---

## 🔨 Bauen, installieren, aktualisieren

```bash
cargo build --release          # → target/release/dkbot
dkbot plugin install <id>      # Plugin installieren (SHA-256-geprüft)
dkbot plugin list              # geladene Plugins
dkbot plugin remove <id>       # deinstallieren
dkbot update                   # Self-Update aus GitHub Releases (statisches musl)
```

- Der erste Start schreibt `~/.config/dakkabot/.config.d/.env` (interaktiv).
- `--non-interactive` für systemd — niemals Prompts.
- Vorkompilierte statische Binaries (x86_64 + aarch64 musl) werden bei jedem
  `v*`-Tag vom Release-Workflow veröffentlicht.

---

## ⚙️ Konfiguration

`~/.config/dakkabot/.config.d/.env` wird zuerst gelesen, dann die Umgebung.

| Variable | Rolle |
|---|---|
| `DISCORD_TOKEN` | Bot-Token (erforderlich) |
| `DISCORD_CLIENT_ID` | Anwendungs-ID (erforderlich für Slash Commands) |
| `DISCORD_OWNER_ID` | Owner-ID — voller Zugriff |
| `DISCORD_GUILD_IDS` | Erlaubte Guilds (kommagetrennt) |
| `DISCORD_ALERT_CHANNEL_ID` | Kanal für Watchdog-Alarme |
| `DISCORD_AUDIT_CHANNEL_ID` | Audit-Kanal (bestätigte Aktionen, Backups…) |
| `ALERT_INTERVAL_MS` | Watchdog-Alarm-Intervall |
| `TARGETS` / `TARGET_HOST` | Zielmaschinen `alias:host` (erste = Standard) |
| `SERVICES` / `SERVICE` | systemd-Units `alias:unit` (erste = Standard) |
| `SSH_USER` `SSH_PASSWORD` `SSH_KEY_PATH` | SSH-Zugang zu Zielen |
| `SSH_KNOWN_HOSTS_FILE` `SSH_TIMEOUT_SECS` | SSH-Härtung |
| `SYSTEMCTL_PREFIX` | z. B. `sudo -n`, wenn root nötig ist |
| `LOG_FILE` `LOG_LINES` | Log-Speicherung / Standard-Zeilenanzahl |
| `SAVES_PATH` `SAVES_COPY_TIMEOUT_SECS` | Spiel-Save-Kopie (siehe 💾) |
| `WATCHDOG_CPU_PCT` `WATCHDOG_RAM_PCT` `WATCHDOG_DISK_PCT` | Watchdog-Schwellen |
| `BOT_LANGUAGE` | `en` / `fr` |

Plugin-spezifische Variablen (`RSDW_API_TOKEN`, `DKBOT_SAVES_PATH`, …) leben
in derselben Datei und sind in der jeweiligen Plugin-README dokumentiert.

---

## 🎮 Kern-Befehle

| Befehl | Rolle |
|---|---|
| `/status` | Echter Maschinenstatus: CPU, RAM, Disk, Dienste |
| `/start` `/stop` `/restart` `/reboot` | Power-Aktionen **mit Bestätigung** |
| `/ping` | Gateway-Latenz |
| `/logs [n]` `/botlogs [n]` | Spiel-Logs / Bot-Logs (tail) |
| `/users` | Systembenutzer (Sitzungen, letzter Login) |
| `/schedule` | Wiederkehrende Aufgaben (systemd-Timer) |
| `/backups` | Spiel-Save-Archive (siehe 💾) |
| `/exec` | Ausführung von Befehlen auf Whitelist |
| `/target` `/server` | Zielmaschinen- / Dienstauswahl |
| `/plugin` | Plugin-Verwaltung (install / list / remove / run) |
| `/help` | Befehlsübersicht |

Alle sensiblen Aktionen sind durch Owner/Operator-Rollen geschützt und
verlangen eine explizite Bestätigung in Discord.

---

## 🧩 Plugins — dynamische Slash Commands

Ein Plugin ist ein `cdylib`, das von der Crate `dkbot-host-api` (ABI v1)
abhängt, eine statische Vtable definiert und `export_plugin!(&VTABLE)` aufruft —
siehe `plugins/dummy-plugin/` als Minimalbeispiel, und die echten Plugins:

- 🐉 **[dkbot-dragonwilds](https://github.com/ApachaiZ/dkbot-dragonwilds)** — 19 Befehle zur Verwaltung eines RuneScape: Dragonwilds-Servers (`/dw …`).
- 💾 **[dkbots-saves](https://github.com/ApachaiZ/dkbots-saves)** — Welt-Save-Archive (`/saves …`).

**Vom Manifest zu Discord, live:**

```json
{"group": "dw", "description": "Dragonwilds-Server-Verwaltung", "commands": [
  {"name": "tp", "description": "Spieler auf einen anderen teleportieren",
   "args": [{"name": "from", "kind": "player", "required": true},
            {"name": "to",   "kind": "player", "required": true}]}
]}
```

Der Host macht aus jedem Manifest-Befehl beim Start einen typisierten Unterbefehl:

- `kind: player|item` → **Autovervollständigung** (das Plugin antwortet auf
  einen `__ac`-Aufruf mit bis zu 25 Vorschlägen — Nicknames, Item-Namen);
- `choices: ["on","off"]` → festes Auswahlmenü;
- `permissions: "admin"` → separate Gruppe `{group}-admin`, nur für
  Guild-Administratoren sichtbar (der Owner sieht sie immer);
- ein Plugin-Ergebnis im Format `{"card": {title, description, tone, fields}}`
  wird als Standard-Discord-Embed gerendert — alles andere fällt auf einen
  JSON-Block zurück (ältere Plugins funktionieren weiter);
- ändert sich ein Manifest zur Laufzeit (z. B. der Dragonwilds-Cheat-Schalter),
  registriert der Host die Guild-Befehle automatisch neu — kein Neustart.

---

## 💾 Spiel-Backups

`/backups` archiviert das Spiel-Save-Verzeichnis (konfigurierbar über
`SAVES_PATH`), lokal aufbewahrt und bereinigt. Das Begleit-Plugin
**dkbots-saves** ergänzt `/saves backup|list|restore` (nur Owner) mit
konfigurierbarem Archivort und Aufbewahrung.

---

## 🔐 Sicherheit

- 🔐 Bot-Token und Geheimnisse in `.env` — niemals im Repo.
- ✅ **Bestätigungs-Prompts** für jede destruktive Aktion.
- 🛡️ Owner-/Operator-Rollen-Schutz für alle sensiblen Befehle.
- 📝 Jede bestätigte Aktion wird im Audit-Kanal und in den Logs protokolliert.
- 🚫 Plugins werden nur aus `~/.config/dakkabot/plugins/` geladen, mit
  SHA-256-Prüfung bei der Installation und versionierter ABI-Prüfung beim Laden.

---

## 🧪 Tests

```bash
cargo test                          # 84 Unit-Tests — kein Netzwerk, keine Dateien außerhalb tmp/
DKBOT_TEST_PLUGINS=/tmp/plugins-test cargo test -- --ignored
                                    # Integration: lädt ECHTE .so-Plugins, prüft
                                    # Manifeste, Autocomplete-Flags, Ordnungsregeln
```

---

## 📄 Lizenz

MIT.
