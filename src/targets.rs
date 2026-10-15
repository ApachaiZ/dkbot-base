//! Parsing et résolution des CIBLES (machines : locale ou SSH) et des
//! SERVICES (unités systemd applicatives — ex. `nginx`, `myservice`).
//!
//! Une « cible » est un couple `(alias, spec)` : l'alias est le nom court
//! utilisé dans Discord (`/status prod`), la spec décrit la machine :
//! - `local` : la machine où tourne le bot ;
//! - `[user@]host[:port]` : une machine distante en SSH.
//!
//! Un « service » est un couple `(alias, unité systemd)` : l'alias est le
//! nom court (`web`), l'unité le nom systemd (`nginx`). Les noms
//! d'unités sont STRICTEMENT validés (`[A-Za-z0-9_.@:\\-]+`) : ils ne
//! traversent jamais une ligne de commande sans cette garantie (injection).

use std::fmt;

/// Une machine ciblée par le bot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSpec {
    pub alias: String,
    pub kind: TargetKind,
}

/// Type de connexion d'une cible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetKind {
    /// La machine locale (celle du bot).
    Local,
    /// Une machine distante en SSH. `user` est le sur-défaut éventuel ;
    /// la config globale `SSH_USER` s'applique sinon.
    Ssh {
        host: String,
        port: u16,
        user: Option<String>,
    },
}

impl TargetSpec {
    /// Nom d'affichage : l'hôte pour une cible SSH, « local » sinon.
    pub fn display(&self) -> String {
        match &self.kind {
            TargetKind::Local => "local".to_string(),
            TargetKind::Ssh { host, .. } => host.clone(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self.kind, TargetKind::Local)
    }
}

/// Détecte une valeur « non définie » : absente, vide, ou placeholder
/// (`YOUR_...`) — le bot refuse de démarrer sur un `.env` d'exemple.
pub fn is_unset(value: Option<&str>) -> bool {
    match value {
        None => true,
        Some(s) => {
            let t = s.trim();
            t.is_empty() || t.starts_with("YOUR_")
        }
    }
}

/// Alias valide : lettres, chiffres, `-` et `_`.
fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Nom d'unité systemd valide (les caractères autorisés par systemd).
pub fn valid_unit(unit: &str) -> bool {
    !unit.is_empty()
        && unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | ':' | '-'))
}

/// Parse `alias1=spec1,alias2=spec2`. Une spec est `local` ou
/// `[user@]host[:port]`. Le premier alias déclaré est la cible par défaut.
/// En l'absence de valeur exploitable, `fallback_host` (TARGET_HOST) donne
/// une cible `default` ; sinon, une cible `default=local` est créée : le
/// cas d'usage principal est la machine locale, et la config le documente.
pub fn parse_targets(raw: Option<&str>, fallback_host: Option<&str>) -> Vec<TargetSpec> {
    let mut map: Vec<TargetSpec> = Vec::new();
    if let Some(raw) = raw.filter(|r| !is_unset(Some(r))) {
        for pair in raw.split(',') {
            let Some(eq) = pair.find('=') else { continue };
            let alias = pair[..eq].trim();
            let spec = pair[eq + 1..].trim();
            if !valid_alias(alias) || map.iter().any(|t| t.alias == alias) {
                continue;
            }
            if let Some(kind) = parse_spec(spec) {
                map.push(TargetSpec {
                    alias: alias.to_string(),
                    kind,
                });
            }
        }
    }
    if map.is_empty() {
        let fallback = fallback_host.filter(|f| !is_unset(Some(f)));
        if let Some(host) = fallback {
            if let Some(kind) = parse_spec(host) {
                map.push(TargetSpec {
                    alias: "default".to_string(),
                    kind,
                });
            }
        }
    }
    if map.is_empty() {
        map.push(TargetSpec {
            alias: "default".to_string(),
            kind: TargetKind::Local,
        });
    }
    map
}

/// Parse une spec de cible : `local` ou `[user@]host[:port]`.
/// `None` si la spec est inexploitable.
pub fn parse_spec(spec: &str) -> Option<TargetKind> {
    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("local") {
        return Some(TargetKind::Local);
    }
    if spec.is_empty() || spec.contains(char::is_whitespace) {
        return None;
    }
    let (user, rest) = match spec.rfind('@') {
        Some(idx) if idx > 0 => (Some(spec[..idx].to_string()), &spec[idx + 1..]),
        _ => (None, spec),
    };
    let (host, port) = parse_host_port(rest)?;
    Some(TargetKind::Ssh { host, port, user })
}

/// Sépare `host[:port]` en acceptant la forme IPv6 entre crochets
/// (`[::1]:22`). Une chaîne IPv6 brute (plusieurs `:`) est prise telle
/// quelle, sans port déchiffrable.
fn parse_host_port(rest: &str) -> Option<(String, u16)> {
    if let Some(stripped) = rest.strip_prefix('[') {
        let close = stripped.find(']')?;
        let host = stripped[..close].to_string();
        let port = match stripped[close + 1..].strip_prefix(':') {
            Some(p) => p.parse::<u16>().ok()?,
            None => 22,
        };
        return Some((host, port));
    }
    if let Some((host, port_str)) = rest.rsplit_once(':') {
        // Un port DOIT être numérique ET l'hôte ne doit pas contenir
        // d'autre `:` (sinon c'est une IPv6 brute, sans port).
        if !host.contains(':')
            && !port_str.is_empty()
            && port_str.chars().all(|c| c.is_ascii_digit())
        {
            let port = port_str.parse::<u16>().ok()?;
            if !host.is_empty() {
                return Some((host.to_string(), port));
            }
        }
    }
    if rest.is_empty() {
        return None;
    }
    Some((rest.to_string(), 22))
}

/// Parse `alias1=unit1,alias2=unit2` (unités systemd validées). En
/// l'absence de valeur exploitable, retombe sur `fallback_unit` (SERVICE)
/// avec l'alias `default`.
pub fn parse_services(raw: Option<&str>, fallback_unit: Option<&str>) -> Vec<(String, String)> {
    let mut map: Vec<(String, String)> = Vec::new();
    if let Some(raw) = raw.filter(|r| !is_unset(Some(r))) {
        for pair in raw.split(',') {
            let Some(eq) = pair.find('=') else { continue };
            let alias = pair[..eq].trim();
            let unit = pair[eq + 1..].trim();
            if !valid_alias(alias) || !valid_unit(unit) || map.iter().any(|(a, _)| a == alias) {
                continue;
            }
            map.push((alias.to_string(), unit.to_string()));
        }
    }
    if map.is_empty() {
        if let Some(f) = fallback_unit.filter(|f| !is_unset(Some(f))) {
            let f = f.trim();
            if valid_unit(f) {
                map.push(("default".to_string(), f.to_string()));
            }
        }
    }
    map
}

/// Résout un alias (éventuellement `None` → cible par défaut) en
/// `(alias, &TargetSpec)`. Erreur explicite si l'alias est inconnu.
pub fn pick_target<'a>(
    targets: &'a [TargetSpec],
    alias: Option<&str>,
) -> Result<(String, &'a TargetSpec), String> {
    let wanted = match alias {
        Some(a) if !is_unset(Some(a)) => a.trim().to_string(),
        _ => targets.first().map(|t| t.alias.clone()).unwrap_or_default(),
    };
    match targets.iter().find(|t| t.alias == wanted) {
        Some(t) => Ok((wanted, t)),
        None => {
            let known: Vec<String> = targets.iter().map(|t| format!("\"{}\"", t.alias)).collect();
            Err(format!(
                "Unknown target \"{wanted}\". Known: {}",
                known.join(", ")
            ))
        }
    }
}

/// Résout un alias (éventuellement `None` → service par défaut) en
/// `(alias, unité)`. Erreur explicite si l'alias est inconnu.
pub fn pick_service<'a>(
    services: &'a [(String, String)],
    alias: Option<&str>,
) -> Result<(&'a str, &'a str), String> {
    let wanted = match alias {
        Some(a) if !is_unset(Some(a)) => a.trim(),
        _ => services.first().map(|(a, _)| a.as_str()).unwrap_or(""),
    };
    match services.iter().find(|(a, _)| a == wanted) {
        Some((a, u)) => Ok((a, u)),
        None => {
            let known: Vec<String> = services.iter().map(|(a, _)| format!("\"{a}\"")).collect();
            Err(format!(
                "Unknown service \"{wanted}\". Known: {}",
                known.join(", ")
            ))
        }
    }
}

impl fmt::Display for TargetSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TargetKind::Local => write!(f, "local"),
            TargetKind::Ssh { host, port, user } => match user {
                Some(u) => write!(f, "{u}@{host}:{port}"),
                None => write!(f, "{host}:{port}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_unset_catches_placeholders() {
        assert!(is_unset(None));
        assert!(is_unset(Some("  ")));
        assert!(is_unset(Some("YOUR_TOKEN")));
        assert!(!is_unset(Some("abc")));
    }

    #[test]
    fn parse_spec_local() {
        assert_eq!(parse_spec("local"), Some(TargetKind::Local));
        assert_eq!(
            parse_spec("root@vps.example.com:2222"),
            Some(TargetKind::Ssh {
                host: "vps.example.com".into(),
                port: 2222,
                user: Some("root".into()),
            })
        );
        assert_eq!(
            parse_spec("1.2.3.4"),
            Some(TargetKind::Ssh {
                host: "1.2.3.4".into(),
                port: 22,
                user: None
            })
        );
        assert_eq!(
            parse_spec("[::1]:2200"),
            Some(TargetKind::Ssh {
                host: "::1".into(),
                port: 2200,
                user: None
            })
        );
        // IPv6 brute : pas de port déchiffrable → port 22, hôte complet.
        assert_eq!(
            parse_spec("fe80::1"),
            Some(TargetKind::Ssh {
                host: "fe80::1".into(),
                port: 22,
                user: None
            })
        );
        assert_eq!(parse_spec("has space"), None);
        assert_eq!(parse_spec(""), None);
    }

    #[test]
    fn parse_targets_first_alias_wins() {
        let t = parse_targets(Some("main=local, prod=root@1.2.3.4, main=1.2.3.4"), None);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].alias, "main");
        assert!(t[0].is_local());
        assert_eq!(t[1].alias, "prod");
    }

    #[test]
    fn parse_targets_falls_back_to_local() {
        let t = parse_targets(None, None);
        assert_eq!(t.len(), 1);
        assert!(t[0].is_local());
        assert_eq!(t[0].alias, "default");
    }

    #[test]
    fn parse_targets_falls_back_to_host() {
        let t = parse_targets(None, Some("vps.example.com"));
        assert_eq!(t.len(), 1);
        assert!(!t[0].is_local());
    }

    #[test]
    fn parse_services_validates_units() {
        let s = parse_services(
            Some("game=myservice, web=nginx; rm -rf /, ok=unit-a.b@c:d"),
            None,
        );
        assert_eq!(
            s,
            vec![
                ("game".to_string(), "myservice".to_string()),
                ("ok".to_string(), "unit-a.b@c:d".to_string()),
            ]
        );
    }

    #[test]
    fn parse_services_falls_back_to_single() {
        let s = parse_services(Some(""), Some("myservice"));
        assert_eq!(s, vec![("default".to_string(), "myservice".to_string())]);
        let s = parse_services(Some(""), Some("bad unit!"));
        assert!(s.is_empty());
    }

    #[test]
    fn pick_defaults_to_first() {
        let t = parse_targets(Some("a=local,b=1.2.3.4"), None);
        assert_eq!(pick_target(&t, None).unwrap().0, "a");
        assert_eq!(pick_target(&t, Some("b")).unwrap().0, "b");
        assert!(pick_target(&t, Some("zz")).is_err());
        let s = parse_services(Some("a=myservice,b=nginx"), None);
        assert_eq!(pick_service(&s, None).unwrap(), ("a", "myservice"));
        assert_eq!(pick_service(&s, Some("b")).unwrap(), ("b", "nginx"));
        assert!(pick_service(&s, Some("zz")).is_err());
    }
}
