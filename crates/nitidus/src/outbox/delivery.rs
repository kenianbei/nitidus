//! Outbox delivery: the startup recovery scan, the per-frame expiry
//! tick, and transport resolution from the account config.

use bevy::prelude::*;
use nitidus_mail::send::{
    OutgoingTransport, SendEnvelope, SmtpConfig, SmtpCredentials, SmtpEncryption,
};
use nitidus_mail::{AccountId, JobId};

use super::{
    OutboxMeta, OutboxState, PendingSend, RETRY_PARK_MS, epoch_ms, outbox_directory,
    remove_file_logged,
};
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
                    auth: crate::config::oauth::resolve_auth(account_config, &config_dir)?,
                }),
            }))
        }
        Some(Outgoing::Sendmail(sendmail)) => Ok(OutgoingTransport::Sendmail {
            command: sendmail.command.clone(),
        }),
        None => anyhow::bail!("account {account} has no outgoing transport configured"),
    }
}

/// Post-send bookkeeping: append the Sent copy (policy resolved at
/// queue time), mark the answered source, then remove every file the
/// entry owned.
pub fn after_send(
    entry: &PendingSend,
    engine: Option<&crate::engine::EngineResource>,
    store: &mut crate::store::MailStore,
) {
    let account = AccountId::new(&entry.meta.account);
    if let Some(engine) = engine {
        if entry.meta.save_sent
            && !entry.meta.sent_folder.is_empty()
            && let Ok(bytes) = std::fs::read(&entry.eml_path)
        {
            let command = nitidus_mail::MailCommand::AppendMessage {
                folder: nitidus_mail::FolderId::new(&entry.meta.sent_folder),
                bytes,
                flags: nitidus_mail::Flags::SEEN,
            };
            if let Err(error) = engine.0.send(&account, command) {
                tracing::warn!("sent copy failed: {error}");
            }
        }
        if let Some((source_account, folder, id)) = &entry.meta.reply_source {
            mark_answered(engine, store, source_account, folder, id);
        }
        if let Some((folder, id)) = &entry.meta.draft_source {
            let delete = nitidus_mail::MailCommand::DeleteMessage {
                folder: nitidus_mail::FolderId::new(folder),
                id: nitidus_mail::EnvelopeId::new(id),
            };
            if let Err(error) = engine.0.send(&account, delete) {
                tracing::warn!("sent-draft removal: {error}");
            }
        }
    }
    remove_file_logged(&entry.eml_path);
    remove_file_logged(&entry.meta_path);
    remove_file_logged(&entry.meta.body_path);
}

fn mark_answered(
    engine: &crate::engine::EngineResource,
    store: &mut crate::store::MailStore,
    account: &str,
    folder: &str,
    id: &str,
) {
    let account = AccountId::new(account);
    let folder = nitidus_mail::FolderId::new(folder);
    let id = nitidus_mail::EnvelopeId::new(id);
    let Some(current) = store
        .envelopes(&account, &folder)
        .iter()
        .find(|envelope| envelope.id == id)
        .map(|envelope| envelope.flags)
    else {
        return;
    };
    let flags = current.with(nitidus_mail::Flags::ANSWERED);
    store.set_flags(&account, &folder, &id, flags);
    let command = nitidus_mail::MailCommand::SetFlags { folder, id, flags };
    if let Err(error) = engine.0.send(&account, command) {
        tracing::warn!("answered flag failed: {error}");
    }
}

/// A failed job stays queued (files intact), parked out of the tick
/// loop; startup retries it afresh.
pub fn fail_send(outbox: &mut OutboxState, job: JobId) -> bool {
    let Some(entry) = outbox
        .0
        .iter_mut()
        .find(|entry| entry.submitted == Some(job))
    else {
        return false;
    };
    entry.submitted = None;
    entry.meta.send_at_epoch_ms = epoch_ms() + RETRY_PARK_MS;
    true
}

/// The account's sent-copy policy, resolved at queue time so the meta
/// file is self-contained.
pub(super) fn sent_policy(world: &World, account: &str) -> (bool, String) {
    let config = world.resource::<crate::config::Config>();
    config
        .accounts
        .iter()
        .find(|candidate| candidate.name == account)
        .map(|account_config| {
            (
                account_config.folders.save_sent,
                account_config.folders.sent.clone(),
            )
        })
        .unwrap_or((false, String::new()))
}
