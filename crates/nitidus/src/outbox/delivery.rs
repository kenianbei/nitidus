//! Outbox delivery: the startup recovery scan, the per-frame expiry
//! tick, and transport resolution from the account config.

use bevy::prelude::*;
use nitidus_mail::AccountId;
use nitidus_mail::send::{
    OutgoingTransport, SendEnvelope, SmtpConfig, SmtpCredentials, SmtpEncryption,
};

use super::{OutboxMeta, OutboxState, PendingSend, RETRY_PARK_MS, epoch_ms, outbox_directory};
use crate::config::account::{Encryption, Outgoing};
use crate::engine::EngineResource;
use crate::status::StatusMessage;

pub(super) fn scan_outbox(world: &mut World) {
    let Ok(directory) = outbox_directory(world) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return;
    };
    let mut recovered = 0usize;
    for entry in entries.flatten() {
        let meta_path = entry.path();
        if meta_path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        match load_entry(&meta_path) {
            Ok(pending) => {
                world.resource_mut::<OutboxState>().0.push(pending);
                recovered += 1;
            }
            Err(error) => tracing::warn!("outbox entry {}: {error:#}", meta_path.display()),
        }
    }
    if recovered > 0 {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .info(format!("outbox: resuming {recovered} queued send(s)"), now);
    }
}

fn load_entry(meta_path: &std::path::Path) -> anyhow::Result<PendingSend> {
    let meta: OutboxMeta = toml::from_str(&std::fs::read_to_string(meta_path)?)?;
    let eml_path = meta_path.with_extension("eml");
    if !eml_path.exists() {
        anyhow::bail!("message file missing for {}", meta_path.display());
    }
    let stem = meta_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_owned();
    Ok(PendingSend {
        stem,
        eml_path,
        meta_path: meta_path.to_path_buf(),
        meta,
        submitted: None,
    })
}

/// Departs due entries: resolves the account's outgoing transport and
/// hands the message to the engine.
pub(super) fn tick_outbox(world: &mut World) {
    let now = epoch_ms();
    let due: Vec<usize> = world
        .resource::<OutboxState>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.submitted.is_none() && entry.meta.send_at_epoch_ms <= now)
        .map(|(index, _)| index)
        .collect();
    for index in due {
        submit_entry(world, index);
    }
}

fn submit_entry(world: &mut World, index: usize) {
    let (account, envelope, eml_path) = {
        let outbox = world.resource::<OutboxState>();
        let Some(entry) = outbox.0.get(index) else {
            return;
        };
        (
            AccountId::new(&entry.meta.account),
            SendEnvelope {
                from: entry.meta.envelope_from.clone(),
                recipients: entry.meta.recipients.clone(),
            },
            entry.eml_path.clone(),
        )
    };
    let outcome = build_transport(world, &account)
        .and_then(|transport| Ok((transport, std::fs::read(&eml_path)?)));
    let now = world.resource::<Time>().elapsed_secs_f64();
    match outcome {
        Ok((transport, bytes)) => {
            let Some(engine) = world.get_resource::<EngineResource>() else {
                return;
            };
            let job = engine.0.next_job();
            engine.0.submit(account, transport, envelope, bytes, job);
            if let Some(entry) = world.resource_mut::<OutboxState>().0.get_mut(index) {
                entry.submitted = Some(job);
            }
        }
        Err(error) => {
            // Push the entry a delay into the future so the error does
            // not repeat every frame; startup retries it afresh.
            if let Some(entry) = world.resource_mut::<OutboxState>().0.get_mut(index) {
                entry.meta.send_at_epoch_ms = epoch_ms() + RETRY_PARK_MS;
            }
            world
                .resource_mut::<StatusMessage>()
                .error(format!("send failed: {error:#}"), now);
        }
    }
}

/// Account config → transport, resolving SMTP credentials on demand.
fn build_transport(world: &World, account: &AccountId) -> anyhow::Result<OutgoingTransport> {
    let config = world.resource::<crate::config::Config>();
    let account_config = config
        .accounts
        .iter()
        .find(|candidate| candidate.name == account.as_str())
        .ok_or_else(|| anyhow::anyhow!("unknown account {account}"))?;
    match &account_config.outgoing {
        Some(Outgoing::Smtp(smtp)) => {
            let config_dir = crate::dirs::config_dir()?;
            let password =
                crate::config::secrets::resolve_password(&account_config.auth, &config_dir)?;
            Ok(OutgoingTransport::Smtp(SmtpConfig {
                host: smtp.host.clone(),
                port: smtp.port,
                encryption: match smtp.encryption {
                    Encryption::Tls => SmtpEncryption::Tls,
                    Encryption::Starttls => SmtpEncryption::StartTls,
                    Encryption::None => SmtpEncryption::None,
                },
                credentials: Some(SmtpCredentials {
                    user: account_config.email.clone(),
                    password,
                }),
            }))
        }
        Some(Outgoing::Sendmail(sendmail)) => Ok(OutgoingTransport::Sendmail {
            command: sendmail.command.clone(),
        }),
        None => anyhow::bail!("account {account} has no outgoing transport configured"),
    }
}
