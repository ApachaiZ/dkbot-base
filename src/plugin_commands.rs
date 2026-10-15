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
            let capped = crate::commands::cap_embed(&result, 3900);
            format!("🐉 `{}` →\n```json\n{capped}\n```", display_name)
        }
        Ok(Err(e)) => e,
        Err(join) => format!("tâche interrompue : {join}"),
    };

    let _ = command
        .edit_response(
            &ctx.http,
            serenity::EditInteractionResponse::new().content(body),
        )
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
