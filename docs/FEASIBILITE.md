# 📡 Lire les données du jeu en cours — retour de faisabilité

> Objectif : le bot doit pouvoir **lire les données du jeu en cours**
> (joueurs connectés, événements, état de la partie…) — via les logs du
> jeu, une API, ou un mod précis. Ce document analyse les trois canaux,
> leur coût, leurs prérequis, et propose un plan par étapes.

---

## 1. Ce que le bot sait DÉJÀ faire (sans rien ajouter)

| Capacité | Déjà là ? | Comment |
| --- | --- | --- |
| Lire les dernières lignes de log du service | ✅ | `/logs` — `journalctl -u dragonwinds` ou `tail` de `LOG_FILE`, avec filtre |
| Savoir si le service tourne / depuis quand | ✅ | `/status` — `ActiveState` + `ActiveEnterTimestampMonotonic` |
| Être alerté si le service crashe | ✅ | watchdog (transition `running → autre`) |
| Savoir si la machine souffre (CPU/RAM/disque) | ✅ | `/status` + watchdog |

Autrement dit : **le canal « logs » est déjà branché**. Ce qui manque, ce
sont (a) une lecture CONTINUE (flux en direct) et (b) une interprétation
structurelle (joueurs, événements) au lieu de texte brut.

---

## 2. Les trois canaux possibles

### Canal A — Logs du service (journald / fichier)

**Principe** : le bot suit les logs en continu (ou les interroge à la
demande) et en extrait des faits — lignes de connexion/déconnexion,
annonces, crashes, sauvegardes.

- **Prérequis** : aucun côté jeu. Le jeu doit seulement écrire dans sa
  sortie standard (journald) ou un fichier de log.
- **Effort côté bot** : faible à moyen.
  - *À la demande* : déjà fait (`/logs`).
  - *En direct* : un collecteur long-lived (équivalent d'un `tail -f` /
    `journalctl -f`) qui suit la sortie et maintient un état en mémoire
    (ex. compteur de joueurs déduit des « X joined / X left ») — ~150–300
    lignes de Rust, en réutilisant les backends local/SSH existants.
- **Limites** : format de log propre à chaque jeu → règles d'extraction à
  écrire ET à maintenir ; données non garanties (un mod peut logger
  autrement) ; latence = celle du flush des logs (souvent OK).
- **Risque** : si le jeu ne loggue pas les événements voulus, rien à
  extraire.

**Verdict : faisable immédiatement, zéro dépendance, à condition de
connaître le format de log du jeu. C'est le point de départ recommandé.**

### Canal B — API / RCON du jeu

**Principe** : interroger directement le processus du jeu (ou un service
annexe) via un protocole machine.

- **Si le jeu expose un RCON** (Source engine, Minecraft avec plugin
  RCON, beaucoup de jeux de survie) : le bot ouvre une connexion TCP et
  envoie des commandes authentifiées (`players`, `status`…).
  - *Effort* : un client RCON générique (~100–200 lignes, tokio) + une
    commande `/game <commande>` + un poll périodique pour `/status` enrichi.
  - *Prérequis* : RCON activé + mot de passe, port accessible depuis le bot
    (localement : localhost ; en SSH : tunnel ou bind public + firewall).
- **Si le jeu expose une API REST/WebSocket** (certains jeux récents, ou
  un wrapper communautaire) : même principe via HTTP.
  - *Effort* : un petit client HTTP dédié (le bot n'a volontairement plus
    de couche HTTP — à réintroduire pour ce module, `reqwest`).
- **Si le jeu n'expose RIEN** : canal inutilisable sans mod (→ Canal C).

**Verdict : le canal le plus fiable et le plus « propre » S'IL existe.
Faisabilité à confirmer : dépend entièrement du jeu.**

### Canal C — Mod / plugin

**Principe** : si le jeu est moddable (Minecraft Forge/Fabric, serveurs
Source avec plugins, jeu sur moteur ouvert…), un petit mod expose une
interface au bot : HTTP local, fichier de statut JSON écrit périodiquement,
ou WebSocket.

- **Ce que le mod peut faire** : tout — joueurs, positions, inventaires,
  événements, commandes à distance. Contrôle maximal.
- **Effort** : le plus élevé. Développer et maintenir un mod (souvent en
  Java/Kotlin/Lua selon le jeu), le distribuer, gérer ses versions à
  chaque mise à jour du jeu.
- **Variante légère** : si le jeu supporte des scripts qui s'exécutent
  périodiquement, un script qui écrit un fichier JSON
  (`/var/lib/dragonwinds/status.json`) suffit — le bot lit ce fichier
  (local ou via SSH), zéro protocole réseau supplémentaire.
- **Prérequis** : chaîne de modding du jeu, compétence dans son écosystème.

**Verdict : à n'envisager qu'après avoir épuisé A et B, sauf si le besoin
(lecture ET contrôle fin du jeu) justifie l'investissement.**

---

## 3. Points à clarifier sur « dragonwinds »

Je ne connais pas ce jeu — la faisabilité des canaux B et C dépend de son
écosystème. Les réponses à ces questions déterminent le plan exact :

1. **Moteur/stack** : quel moteur (Unity, Java, Node, custom…) ? Le serveur
   est-il un binaire seul, une JVM, un conteneur ?
2. **Logs** : où écrit-il ? (`stdout`/journald, ou un fichier — quel
   format ?) Peut-on voir un extrait de log réel ?
3. **Protocoles** : expose-t-il un RCON, une console de commandes, une API
   HTTP, un WebSocket ? (port, protocole, authentification)
4. **Moddabilité** : plugins/mods officiels ou communautaires ? Un système
   de scripts serveur (lua/python) ?
5. **Besoin réel** : quelles données exactement ? (juste « combien de
   joueurs », ou des événements détaillés ? lecture seule ou aussi des
   commandes ?)

---

## 4. Plan recommandé (par étapes, indépendant du jeu)

**Étape 0 — Déjà livrée.** `/status`, `/start`…`/restart`, `/reboot`,
`/logs` (journald/fichier), watchdog.

**Étape 1 — `/logs` en direct (canal A, faible effort).**
Un suiveur de logs (`journalctl -f` ou `tail -F`) branché sur un salon ou
exposé par `/logs live`, avec les mêmes backends local/SSH. Sert de base à
tout le reste et valide le canal.

**Étape 2 — Extraction structurée (canal A ou B).**
Selon les réponses aux questions ci-dessus :
- si RCON/API existe → client dédié + `/status` enrichi (joueurs en direct)
  et `/game <commande>` (opérateurs uniquement) ;
- sinon → règles d'extraction sur les logs (compteur de joueurs, crashs).

**Étape 3 — Mod/script (canal C, seulement si nécessaire).**
Fichier de statut JSON écrit par le jeu → lu par le bot (chemin le plus
simple), ou WebSocket si l'interactivité le justifie.

Chaque étape est **incrémentale et non destructive** : l'étape 1 tient en
une commande, l'étape 2 en un module, l'étape 3 est un projet à part entière.

---

## 5. Estimation d'effort (à la loupe, bot Rust existant)

| Étape | Effort | Risque principal |
| --- | --- | --- |
| 1. `/logs` live | ~0,5 j | aucun (réutilise les backends) |
| 2a. Client RCON/API | 1–2 j | protocole du jeu mal documenté |
| 2b. Extraction de logs | 1–3 j | format de log changeant |
| 3. Mod/plugin | 3 j → semaines | écosystème de modding |

**Recommandation : commencer par l'étape 1 dès la validation du bot, puis
trancher 2a vs 2b avec les réponses de la section 3.**
