//! Sous-commandes de `/users` (owner uniquement) : add / remove / list / clear.
//!
//! Chaque mutation est confirmée (boutons), journalisée (audit) et appliquée
//! via [`State::save_users`] (fichier chiffré = source de vérité).

use poise::serenity_prelude as serenity;
use serenity::{CreateEmbed, UserId};

use crate::commands::{reply, Context, Error};
use crate::embeds::{card, Tone};
use crate::i18n::fill;
use crate::logger;

const PAGE_SIZE: usize = 20;

/// Garde d'accès : seul le propriétaire gère les opérateurs.
pub async fn ensure_owner(ctx: &Context<'_>) -> Result<bool, Error> {
    let data = ctx.data();
    if data.state.is_owner(&ctx.author().id.to_string()) {
        return Ok(true);
    }
    let c = data.catalog.manage.access_denied;
    ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
        .await?;
    Ok(false)
}

/// Nom d'affichage d'un membre (cache → HTTP → ID).
async fn member_name(ctx: &Context<'_>, target: &UserId) -> String {
    let cache = target.to_user(ctx.serenity_context()).await;
    if let Ok(user) = cache {
        if let Some(guild) = ctx.guild_id() {
            if let Ok(member) = guild.member(ctx.http(), target).await {
                return member.display_name().to_string();
            }
        }
        return user.name;
    }
    target.to_string()
}

/// ID d'une sélection « membre » (le sélecteur renvoie un ID numérique).
fn parse_target_id(raw: &str) -> Option<UserId> {
    let id = raw.trim();
    if !id.chars().all(|c| c.is_ascii_digit()) || id.is_empty() {
        return None;
    }
    id.parse::<u64>().ok().map(UserId::new)
}

/// Autocomplétion du sélecteur de membre.
pub async fn member_autocomplete(
    ctx: Context<'_>,
    partial: &str,
    subcommand: &str,
) -> Vec<serenity::AutocompleteChoice> {
    crate::members::member_choices(&ctx, partial, subcommand)
        .await
        .unwrap_or_default()
}

async fn edit_card(
    ctx: &Context<'_>,
    embed: CreateEmbed,
    gif: Option<&crate::assets::Gif>,
) -> Result<(), Error> {
    use serenity::EditInteractionResponse;
    let edit = match gif {
        Some(g) if g.attachment.is_some() => {
            let embed = embed.image(g.image_url.as_deref().unwrap_or_default());
            EditInteractionResponse::new()
                .embed(embed)
                .new_attachment(g.attachment.as_ref().unwrap().clone())
                .components(Vec::new())
        }
        _ => EditInteractionResponse::new()
            .embed(embed)
            .clear_attachments()
            .components(Vec::new()),
    };
    crate::confirm::edit_original(ctx, edit).await?;
    Ok(())
}

/// `/users add` — autorise un opérateur (avec confirmation).
pub async fn add(ctx: Context<'_>, member: String) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_owner(&ctx).await? {
        return Ok(());
    }
    let catalog = ctx.data().catalog;
    let Some(target_id) = parse_target_id(&member) else {
        let c = catalog.manage.invalid;
        ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
            .await?;
        return Ok(());
    };
    let owner = ctx.author().id;
    if target_id == owner {
        let c = catalog.manage.invalid;
        ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    let username = member_name(&ctx, &target_id).await;
    let users = ctx.data().state.get_users();
    if users.contains(&target_id.to_string()) {
        let c = catalog.manage.no_change;
        ctx.send(reply(card(c.title, c.body_add, Tone::Info, None)))
            .await?;
        return Ok(());
    }

    let c = catalog.manage.confirm;
    let body = fill(c.body, &[("verb", c.verb_add), ("username", &username)]);
    let (_, confirmed) =
        crate::confirm::confirm(&ctx, card(c.title, &body, Tone::Wait, None)).await?;
    if !confirmed {
        return Ok(());
    }

    let mut next = ctx.data().state.get_users();
    next.push(target_id.to_string());
    ctx.data().state.save_users(next);
    logger::info(fill(
        catalog.audit.operator_granted,
        &[
            ("username", username.as_str()),
            ("target", &target_id.to_string()),
            ("ownerId", &owner.to_string()),
        ],
    ));
    let c = catalog.manage.done;
    let body = fill(c.body_add, &[("username", &username)]);
    let gif = crate::assets::gif_image("success");
    edit_card(&ctx, card(c.title, &body, Tone::Good, None), Some(&gif)).await?;
    Ok(())
}

/// `/users remove` — révoque un opérateur (avec confirmation).
pub async fn remove(ctx: Context<'_>, member: String) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_owner(&ctx).await? {
        return Ok(());
    }
    let catalog = ctx.data().catalog;
    let Some(target_id) = parse_target_id(&member) else {
        let c = catalog.manage.invalid;
        ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
            .await?;
        return Ok(());
    };
    let owner = ctx.author().id;
    if target_id == owner {
        let c = catalog.manage.invalid;
        ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    let username = member_name(&ctx, &target_id).await;
    let users = ctx.data().state.get_users();
    if !users.contains(&target_id.to_string()) {
        let c = catalog.manage.no_change;
        ctx.send(reply(card(c.title, c.body_remove, Tone::Info, None)))
            .await?;
        return Ok(());
    }

    let c = catalog.manage.confirm;
    let body = fill(c.body, &[("verb", c.verb_remove), ("username", &username)]);
    let (_, confirmed) =
        crate::confirm::confirm(&ctx, card(c.title, &body, Tone::Wait, None)).await?;
    if !confirmed {
        return Ok(());
    }

    let next: Vec<String> = ctx
        .data()
        .state
        .get_users()
        .into_iter()
        .filter(|u| u != &target_id.to_string())
        .collect();
    ctx.data().state.save_users(next);
    logger::info(fill(
        catalog.audit.operator_revoked,
        &[
            ("username", username.as_str()),
            ("target", &target_id.to_string()),
            ("ownerId", &owner.to_string()),
        ],
    ));
    let c = catalog.manage.done;
    let body = fill(c.body_remove, &[("username", &username)]);
    let gif = crate::assets::gif_image("success");
    edit_card(&ctx, card(c.title, &body, Tone::Good, None), Some(&gif)).await?;
    Ok(())
}

/// `/users list` — liste paginée des opérateurs.
pub async fn list(ctx: Context<'_>, page: Option<usize>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_owner(&ctx).await? {
        return Ok(());
    }
    let data = ctx.data();
    let catalog = data.catalog;
    let users = data.state.get_users();
    let owner_name = member_name(
        &ctx,
        &UserId::new(data.cfg.owner_id.parse().unwrap_or_default()),
    )
    .await;
    let owner_line = fill(catalog.manage.list.header, &[("owner", &owner_name)]);

    let page = page.unwrap_or(0);
    let total_pages = (users.len().saturating_sub(1)) / PAGE_SIZE + 1;
    let page = page.min(total_pages.saturating_sub(1));
    let start = page * PAGE_SIZE;
    let slice = &users[start..(start + PAGE_SIZE).min(users.len())];

    let mut lines: Vec<String> = vec![owner_line];
    for id in slice {
        let name = member_name(&ctx, &UserId::new(id.parse().unwrap_or_default())).await;
        lines.push(fill(catalog.manage.list.entry, &[("name", &name)]));
    }

    let title: String = if users.len() == 1 {
        catalog.manage.list.title_one.to_string()
    } else {
        fill(
            catalog.manage.list.title_many,
            &[
                ("page", &(page + 1).to_string()),
                ("totalPages", &total_pages.to_string()),
            ],
        )
    };
    let body = lines.join("\n");
    if page == 0 {
        ctx.send(reply(card(&title, &body, Tone::Info, None)))
            .await?;
    } else {
        ctx.send(
            poise::CreateReply::default()
                .embed(card(&title, &body, Tone::Info, None))
                .ephemeral(true)
                .allowed_mentions(serenity::CreateAllowedMentions::new()),
        )
        .await?;
    }
    Ok(())
}

/// `/users clear` — révoque tous les opérateurs (avec confirmation).
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_owner(&ctx).await? {
        return Ok(());
    }
    let catalog = ctx.data().catalog;
    let c = catalog.manage.clear;
    let (_, confirmed) = crate::confirm::confirm(
        &ctx,
        card(c.confirm_title, c.confirm_body, Tone::Wait, None),
    )
    .await?;
    if !confirmed {
        return Ok(());
    }
    let owner = ctx.author().id;
    ctx.data().state.save_users(Vec::new());
    logger::info(fill(
        catalog.audit.operators_cleared,
        &[("ownerId", &owner.to_string())],
    ));
    let gif = crate::assets::gif_image("success");
    edit_card(
        &ctx,
        card(c.done_title, c.done_body, Tone::Good, None),
        Some(&gif),
    )
    .await?;
    Ok(())
}
