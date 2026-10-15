//! Commandes slash DYNAMIQUES des plugins : les manifestes des plugins
//! deviennent de vraies commandes Discord (/dw status, /dw tp…), enregistrées
//! par l'hôte au démarrage — installer un plugin fait apparaître ses commandes,
//! sans recompilation du cœur.

use std::sync::Arc;

use poise::serenity_prelude as serenity;

use crate::plugin::{ManifestArg, PluginHost};
use crate::state::State;

/// Construit un `CreateCommand` (groupe) par plugin, avec une subcommand par
/// commande déclarée et ses options typées.
pub fn build_groups(plugins: &PluginHost) -> Vec<serenity::CreateCommand> {
    let mut groups = Vec::new();
    for (plugin_name, manifest) in plugins.manifests() {
        if manifest.group.is_empty() || manifest.commands.is_empty() {
            continue;
        }
        let mut group = serenity::CreateCommand::new(manifest.group.clone());
        let description = if manifest.description.is_empty() {
            format!("Commandes du plugin {plugin_name}")
        } else {
            manifest.description.clone()
        };
        group = group.description(description);
        for command in manifest.commands {
            let mut sub = serenity::CreateCommandOption::new(
                serenity::CommandOptionType::SubCommand,
                command.name.clone(),
                command.description.clone(),
            );
            for arg in command.args {
                sub = sub.add_sub_option(option_from_manifest(&arg));
            }
            group = group.add_option(sub);
        }
        groups.push(group);
    }
    groups
}

fn option_from_manifest(arg: &ManifestArg) -> serenity::CreateCommandOption {
    let kind = match arg.kind.as_str() {
        "int" => serenity::CommandOptionType::Integer,
        "bool" => serenity::CommandOptionType::Boolean,
        _ => serenity::CommandOptionType::String,
    };
    serenity::CreateCommandOption::new(kind, arg.name.clone(), arg.description.clone())
        .required(arg.required)
}

/// Rendu d'un résultat de plugin : carte (convention `{"card": …}`) quand le
/// plugin la fournit, sinon bloc JSON brut plafonné. Partagé par les slash
/// commands dynamiques et la commande texte `!plugin run`.
pub fn render_result(fallback_title: &str, result: &str) -> serenity::CreateEmbed {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(result) else {
        let body = crate::commands::cap_embed(result, 3900);
        return crate::embeds::card(fallback_title, &body, crate::embeds::Tone::Good, None);
    };
    if let Some(card) = value.get("card").and_then(|c| c.as_object()) {
        return card_to_embed(card);
    }
    let body = crate::commands::cap_embed(&format!("```json\n{result}\n```"), 3900);
    crate::embeds::card(fallback_title, &body, crate::embeds::Tone::Good, None)
}

fn card_to_embed(card: &serde_json::Map<String, serde_json::Value>) -> serenity::CreateEmbed {
    let title = card.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let description = card
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tone = match card.get("tone").and_then(|v| v.as_str()).unwrap_or("info") {
        "good" => crate::embeds::Tone::Good,
        "bad" => crate::embeds::Tone::Bad,
        "wait" => crate::embeds::Tone::Wait,
        _ => crate::embeds::Tone::Info,
    };
    let fields: Vec<(String, String, bool)> = card
        .get("fields")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let name = f.get("name")?.as_str()?.to_string();
                    let value = f.get("value")?.as_str()?.to_string();
                    let inline = f.get("inline").and_then(|b| b.as_bool()).unwrap_or(false);
                    Some((name, value, inline))
                })
                .collect()
        })
        .unwrap_or_default();
    let footer = card.get("footer").and_then(|v| v.as_str());
    crate::embeds::card_with_fields(title, description, tone, fields, footer)
}

/// Gère une interaction de plugin (appelé depuis le hook d'événements de
/// poise) : renvoie `true` si l'interaction a été consommée.
pub async fn handle_plugin_interaction(
    ctx: &serenity::Context,
    interaction: &serenity::Interaction,
    plugins: &Arc<PluginHost>,
    state: &State,
) -> bool {
    let serenity::Interaction::Command(command) = interaction else {
        return false;
    };
    let options = command.data.options();
    let Some(first) = options.first() else {
        return false;
    };
    let serenity::ResolvedValue::SubCommand(sub_options) = &first.value else {
        return false;
    };

    let group = command.data.name.clone();
    let Some((plugin_name, _)) = plugins
        .manifests()
        .into_iter()
        .find(|(_, m)| m.group == group)
    else {
        return false;
    };

    // Arguments de la sous-commande → JSON.
    let mut args = serde_json::Map::new();
    for option in sub_options {
        let value = match &option.value {
            serenity::ResolvedValue::String(s) => serde_json::json!(s),
            serenity::ResolvedValue::Integer(i) => serde_json::json!(i),
            serenity::ResolvedValue::Boolean(b) => serde_json::json!(b),
            _ => continue,
        };
        args.insert(option.name.to_string(), value);
    }

    let caller = serde_json::json!({
        "id": command.user.id.to_string(),
        "is_owner": state.is_owner(&command.user.id.to_string()),
        "roles": command
            .member
            .as_ref()
            .map(|m| m.roles.iter().map(|r| r.get().to_string()).collect::<Vec<_>>())
            .unwrap_or_default(),
    });
    let call = serde_json::json!({ "caller": caller, "args": args }).to_string();
    // Nom de la sous-commande COPIÉ (les références locales ne survivent
    // pas au spawn_blocking 'static).
    let command_name: String = first.name.to_string();

    // Réponse DIFFÉRÉE immédiate (l'exécution du plugin peut prendre des
    // secondes — jamais de timeout Discord à 3 s).
    let _ = command
        .create_response(
            &ctx.http,
            serenity::CreateInteractionResponse::Defer(
                serenity::CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await;

    let plugins = plugins.clone();
    let display_name = command_name.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        plugins.run(&plugin_name, &command_name, &call)
    })
    .await;

    let body = match outcome {
        Ok(Ok(result)) => {
            crate::plugin_commands::render_result(&format!("`/{group} {display_name}`"), &result)
        }
        Ok(Err(e)) => crate::embeds::card(
            &format!("🐉 `/{group} {display_name}`"),
            &format!("Erreur du plugin : {e}"),
            crate::embeds::Tone::Bad,
            None,
        ),
        Err(join) => crate::embeds::card(
            &format!("🐉 `/{group} {display_name}`"),
            &format!("Tâche interrompue : {join}"),
            crate::embeds::Tone::Bad,
            None,
        ),
    };

    let _ = command
        .edit_response(&ctx.http, serenity::EditInteractionResponse::new().add_embed(body))
        .await;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::parse_manifest;

    #[test]
    fn builds_group_from_manifest() {
        let raw = r#"{"group":"dw","description":"Admin","commands":[
            {"name":"tp","description":"Téléporte","args":[{"name":"from","kind":"string","required":true}]},
            {"name":"top","description":"Classement","args":[{"name":"n","kind":"int"}]}]}"#;
        let manifest = parse_manifest(raw, "dragonwilds");
        let mut groups = Vec::new();
        let mut group = serenity::CreateCommand::new(manifest.group.clone());
        group = group.description(manifest.description.clone());
        for command in manifest.commands {
            let mut sub = serenity::CreateCommandOption::new(
                serenity::CommandOptionType::SubCommand,
                command.name.clone(),
                command.description.clone(),
            );
            for arg in command.args {
                sub = sub.add_sub_option(option_from_manifest(&arg));
            }
            group = group.add_option(sub);
        }
        groups.push(group);
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn renders_card_from_plugin_json() {
        let raw = r#"{"card":{"title":"🐉 Statut du serveur","description":"en ligne","tone":"good","fields":[{"name":"👥 Joueurs","value":"2","inline":true},{"name":"🌍 Monde","value":"ApaWorld","inline":true}]}}"#;
        let embed = render_result("fallback", raw);
        let json = serde_json::to_value(&embed).expect("sérialisable");
        assert_eq!(json["title"], "🐉 Statut du serveur");
        assert_eq!(json["description"], "en ligne");
        let fields = json["fields"].as_array().expect("champs");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0]["name"], "👥 Joueurs");
        assert_eq!(fields[0]["value"], "2");
    }

    #[test]
    fn falls_back_to_json_block_without_card() {
        let raw = "{\"count\":0,\"players\":[]}";
        let embed = render_result("`/dw players`", raw);
        let json = serde_json::to_value(&embed).expect("sérialisable");
        assert_eq!(json["title"], "`/dw players`");
        assert!(json["description"]
            .as_str()
            .expect("description")
            .contains("```json"));
    }

    #[test]
    fn handles_non_json_result() {
        let embed = render_result("x", "réponse illisible");
        let json = serde_json::to_value(&embed).expect("sérialisable");
        assert_eq!(json["description"], "réponse illisible");
    }

    #[test]
    fn option_kinds_map_to_discord_types() {
        let arg = ManifestArg {
            name: "n".into(),
            description: String::new(),
            kind: "int".into(),
            required: true,
        };
        let option = option_from_manifest(&arg);
        let built = serenity::CreateCommand::new("g").add_option(option);
        assert!(serde_json::to_value(&built).is_ok());
    }
}
