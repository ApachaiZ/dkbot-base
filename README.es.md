<div align="center">

# 🎛️ DakkaBot — dkbot-base

### Control fino del VPS desde Discord — un único binario Rust nativo, cero dependencias en tiempo de ejecución 🦀

[![Rust](https://img.shields.io/badge/Rust-%E2%89%A5%201.85-orange?logo=rust)](https://rust-lang.org)
[![serenity](https://img.shields.io/badge/serenity-0.12-5865F2?logo=discord)](https://crates.io/crates/serenity)
[![poise](https://img.shields.io/badge/poise-0.6-5865F2)](https://crates.io/crates/poise)
[![Licencia](https://img.shields.io/badge/licencia-MIT-blue)](#-licencia)
[![CI](https://img.shields.io/badge/CI-fmt%20%2B%20clippy%20%2B%20tests-green)](https://github.com/ApachaiZ/dkbot-base/actions)

**Estado real de las máquinas** · acciones de energía seguras · watchdog · backups ·
planificador · host de plugins · i18n en/fr · **slash commands dinámicas desde manifiestos de plugins**

> 🌐 Idiomas: 🇬🇧 [English](README.md) · 🇫🇷 [Français](README.fr.md) · 🇩🇪 [Deutsch](README.de.md) · 🇪🇸 Español

</div>

---

## 📖 Índice

1. [🚀 TL;DR — listo en 30 segundos](#-tldr--listo-en-30-segundos)
2. [🎯 Qué es](#-qué-es)
3. [🏗️ Arquitectura (infografía)](#️-arquitectura-infografía)
4. [🔨 Compilar, instalar, actualizar](#-compilar-instalar-actualizar)
5. [⚙️ Configuración](#️-configuración)
6. [🎮 Comandos del núcleo](#-comandos-del-núcleo)
7. [🧩 Plugins — slash commands dinámicas](#-plugins--slash-commands-dinámicas)
8. [💾 Backups del juego](#-backups-del-juego)
9. [🔐 Seguridad](#-seguridad)
10. [🧪 Tests](#-tests)
11. [📄 Licencia](#-licencia)

---

## 🚀 TL;DR — listo en 30 segundos

```bash
# 1. Único requisito: Rust ≥ 1.85
cargo build --release

# 2. Primer arranque: el setup interactivo escribe ~/.config/dakkabot/.config.d/.env
./target/release/dkbot

# 3. Producción (systemd): nunca pide confirmación por teclado
./target/release/dkbot --non-interactive
```

> ⚠️ **Requisito del portal de desarrolladores**: activa el intent privilegiado
> **Server Members Intent** (portal → Bot → Privileged Gateway Intents).
> Sin él, la pasarela de Discord rechaza la conexión.

---

## 🎯 Qué es

**dkbot-base** es el núcleo de DakkaBot: un bot de Discord que controla una o
varias máquinas Linux (el VPS que aloja tu servidor de juego, o hosts remotos
por SSH) — más un **host de plugins**: instala un plugin (`.so`, ABI v1) y sus
comandos aparecen en Discord como slash commands reales, sin recompilar el núcleo.

## 🏗️ Arquitectura (infografía)

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
        │  █ watchdog · i18n en/fr · auditoría · confirmaciones █  │
        │  █ host de plugins (ABI v1) · slash desde manifiestos  █  │
        │  ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀  │
        │       │ plugins/*.so ─── ABI v1 ───► dkbot-dragonwilds   │
        │       │                              dkbots-saves        │
        └───────┼──────────────────────────────────────────────────┘
                │ SSH / systemctl
        ┌───────▼──────────────┐   ┌───────────────────────────────┐
        │  VPS (local o remoto) │◄──│  Servidor de juego (mod LD_PRELOAD)│
        │  servicios, disco…    │   │  API REST en 127.0.0.1:8080   │
        └──────────────────────┘   └───────────────────────────────┘
```

- **Los plugins están desacoplados**: cada uno vive en su propio repositorio,
  se instala como `.so` precompilado (verificado con SHA-256) y declara sus
  comandos en un **manifiesto JSON** — el host convierte ese manifiesto en
  slash commands reales de Discord al arrancar.
- **Política ABI**: un bump mayor de `ABI_VERSION` = los plugins deben
  recompilarse; un plugin incompatible se rechaza al cargar, nunca se invoca.

---

## 🔨 Compilar, instalar, actualizar

```bash
cargo build --release          # → target/release/dkbot
dkbot plugin install <id>      # instalar un plugin (SHA-256 verificado)
dkbot plugin list              # plugins cargados
dkbot plugin remove <id>       # desinstalar
dkbot update                   # auto-actualización desde releases de GitHub (musl estático)
```

- El primer arranque escribe `~/.config/dakkabot/.config.d/.env` (interactivo).
- `--non-interactive` para systemd — nunca prompts.
- Binarios estáticos precompilados (x86_64 + aarch64 musl) se publican en cada
  tag `v*` mediante el workflow de release.

---

## ⚙️ Configuración

`~/.config/dakkabot/.config.d/.env` se lee primero, luego el entorno.

| Variable | Rol |
|---|---|
| `DISCORD_TOKEN` | Token del bot (obligatorio) |
| `DISCORD_CLIENT_ID` | ID de aplicación (obligatorio para slash commands) |
| `DISCORD_OWNER_ID` | ID del propietario — acceso total |
| `DISCORD_GUILD_IDS` | Guilds autorizadas (separadas por comas) |
| `DISCORD_ALERT_CHANNEL_ID` | Canal de alertas del watchdog |
| `DISCORD_AUDIT_CHANNEL_ID` | Canal de auditoría (acciones confirmadas, backups…) |
| `ALERT_INTERVAL_MS` | Intervalo de alerta del watchdog |
| `TARGETS` / `TARGET_HOST` | Máquinas objetivo `alias:host` (la primera es la predeterminada) |
| `SERVICES` / `SERVICE` | Unidades systemd `alias:unidad` (la primera es la predeterminada) |
| `SSH_USER` `SSH_PASSWORD` `SSH_KEY_PATH` | Acceso SSH a los objetivos |
| `SSH_KNOWN_HOSTS_FILE` `SSH_TIMEOUT_SECS` | Endurecimiento SSH |
| `SYSTEMCTL_PREFIX` | p. ej. `sudo -n` cuando se requiere root |
| `LOG_FILE` `LOG_LINES` | Almacenamiento de logs / líneas por defecto |
| `SAVES_PATH` `SAVES_COPY_TIMEOUT_SECS` | Copia de saves del juego (ver 💾) |
| `WATCHDOG_CPU_PCT` `WATCHDOG_RAM_PCT` `WATCHDOG_DISK_PCT` | Umbrales del watchdog |
| `BOT_LANGUAGE` | `en` / `fr` |

Las variables específicas de plugins (`RSDW_API_TOKEN`, `DKBOT_SAVES_PATH`, …)
viven en el mismo archivo y están documentadas en el README de cada plugin.

---

## 🎮 Comandos del núcleo

| Comando | Rol |
|---|---|
| `/status` | Estado real de la máquina: CPU, RAM, disco, servicios |
| `/start` `/stop` `/restart` `/reboot` | Acciones de energía **con confirmación** |
| `/ping` | Latencia de la pasarela |
| `/logs [n]` `/botlogs [n]` | Logs del juego / logs del bot (tail) |
| `/users` | Usuarios del sistema (sesiones, último login) |
| `/schedule` | Tareas recurrentes (timers de systemd) |
| `/backups` | Archivos de saves del juego (ver 💾) |
| `/exec` | Ejecución de comandos en lista blanca |
| `/target` `/server` | Selección de máquina / servicio |
| `/plugin` | Gestión de plugins (install / list / remove / run) |
| `/help` | Resumen de comandos |

Todas las acciones sensibles están protegidas por roles owner/operador y
requieren confirmación explícita en Discord.

---

## 🧩 Plugins — slash commands dinámicas

Un plugin es un `cdylib` que depende del crate `dkbot-host-api` (ABI v1),
define su vtable estática y llama a `export_plugin!(&VTABLE)` — ver
`plugins/dummy-plugin/` como ejemplo mínimo, y los plugins reales:

- 🐉 **[dkbot-dragonwilds](https://github.com/ApachaiZ/dkbot-dragonwilds)** — 19 comandos para administrar un servidor RuneScape: Dragonwilds (`/dw …`).
- 💾 **[dkbots-saves](https://github.com/ApachaiZ/dkbots-saves)** — archivos de saves del mundo (`/saves …`).

**Del manifiesto a Discord, en vivo:**

```json
{"group": "dw", "description": "Administración del servidor Dragonwilds", "commands": [
  {"name": "tp", "description": "Teletransportar un jugador sobre otro",
   "args": [{"name": "from", "kind": "player", "required": true},
            {"name": "to",   "kind": "player", "required": true}]}
]}
```

El host convierte cada comando del manifiesto en un subcomando tipado al arrancar:

- `kind: player|item` → **autocompletado** (el plugin responde a una llamada
  `__ac` con hasta 25 sugerencias — apodos, nombres de objetos);
- `choices: ["on","off"]` → menú de selección fijo;
- `permissions: "admin"` → grupo separado `{grupo}-admin` visible solo para
  los administradores del guild (el propietario lo ve siempre);
- un resultado de plugin con forma `{"card": {title, description, tone, fields}}`
  se renderiza como tarjeta estándar de Discord — cualquier otra cosa cae a
  un bloque JSON (los plugins antiguos siguen funcionando);
- si un manifiesto cambia en tiempo de ejecución (p. ej. el interruptor de
  cheats de dragonwilds), el host re-registra los comandos del guild
  automáticamente — sin reiniciar.

---

## 💾 Backups del juego

`/backups` archiva el directorio de saves del juego (configurable vía
`SAVES_PATH`), conservado y podado localmente. El plugin compañero
**dkbots-saves** añade `/saves backup|list|restore` (solo propietario) con
ubicación de archivo y retención configurables.

---

## 🔐 Seguridad

- 🔐 Token del bot y secretos en `.env` — nunca en el repositorio.
- ✅ **Prompts de confirmación** para cada acción destructiva.
- 🛡️ Control por roles owner / operador en todos los comandos sensibles.
- 📝 Cada acción confirmada se audita en el canal de auditoría y en los logs.
- 🚫 Los plugins solo se cargan desde `~/.config/dakkabot/plugins/`, con
  verificación SHA-256 en la instalación y control de ABI versionado al cargar.

---

## 🧪 Tests

```bash
cargo test                          # 84 tests unitarios — sin red, sin archivos fuera de tmp/
DKBOT_TEST_PLUGINS=/tmp/plugins-test cargo test -- --ignored
                                    # integración: carga plugins .so REALES, verifica
                                    # manifiestos, flags de autocompletado, reglas de orden
```

---

## 📄 Licencia

MIT.
