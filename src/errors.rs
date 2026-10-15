//! Erreurs classifiées des backends (local / SSH) + erreur de configuration.
//!
//! La classification sert DEUX garanties du bot :
//! 1. les messages d'erreur sont ACTIONNABLES (auth SSH, réseau, commande
//!    refusée…) au lieu d'un fourre-tout « vérifiez la config » ;
//! 2. [`may_have_been_transmitted`] décide si une action power a PU partir
//!    malgré l'erreur — le bot ne renvoie alors JAMAIS une seconde commande,
//!    il bascule sur la vérification d'état / le suivi silencieux.

use crate::i18n::{fill, Catalog};

/// Catégorie d'échec d'un backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendErrorKind {
    /// La commande n'a pas répondu à temps (elle a PU être appliquée).
    Timeout,
    /// Impossible de joindre la machine (rien n'est parti).
    Network,
    /// Authentification SSH refusée (rien n'est parti).
    Auth,
    /// La commande a été reçue et REFUSÉE (exit non nul) : rien ne sera
    /// appliqué — pas besoin de suivi.
    Rejected,
    /// Autre échec (message libre).
    Other,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct BackendError {
    pub kind: BackendErrorKind,
    /// La commande a-t-elle pu être livrée et exécutée malgré l'erreur ?
    pub sent: bool,
    pub message: String,
}

impl BackendError {
    pub fn new(kind: BackendErrorKind, sent: bool, message: impl Into<String>) -> Self {
        Self {
            kind,
            sent,
            message: message.into(),
        }
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::new(BackendErrorKind::Other, false, message)
    }

    /// L'action a-t-elle pu partir ? Si oui, l'appelant ne doit JAMAIS
    /// renvoyer une seconde commande — uniquement vérifier l'état réel.
    pub fn may_have_been_transmitted(&self) -> bool {
        self.sent || self.kind == BackendErrorKind::Timeout
    }
}

/// Contexte d'une erreur : une action power ajoute la mention « aucun
/// retry » au message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorContext {
    Power,
    Other,
}

/// Message localisé pour une [`BackendError`].
pub fn error_text(error: &BackendError, context: ErrorContext, catalog: &Catalog) -> String {
    let suffix = if context == ErrorContext::Power {
        catalog.errors.power_suffix
    } else {
        ""
    };
    let (template, message) = match error.kind {
        BackendErrorKind::Auth => (catalog.errors.ssh_auth, String::new()),
        BackendErrorKind::Network => (catalog.errors.ssh_network, String::new()),
        BackendErrorKind::Timeout => (
            if context == ErrorContext::Power {
                catalog.errors.ssh_timeout_sent
            } else {
                catalog.errors.ssh_timeout
            },
            String::new(),
        ),
        BackendErrorKind::Rejected => (catalog.errors.command_rejected, error.message.clone()),
        BackendErrorKind::Other => (catalog.errors.generic, error.message.clone()),
    };
    fill(template, &[("message", &message), ("suffix", suffix)])
}

/// Erreur de CONFIGURATION → code de sortie 78 (EX_CONFIG) : systemd et tout
/// superviseur ne redémarrent PAS en boucle sur une config invalide.
#[derive(Debug, thiserror::Error)]
#[error("{msg}")]
pub struct ExitError {
    pub msg: String,
    pub code: i32,
}

impl ExitError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            code: 78,
        }
    }
}
