//! The composer: a single compose session flowing headers form →
//! `$EDITOR` → review screen. The session's body lives in a
//! crash-surviving file under the state dir; send/postpone are staged
//! stubs until the 1c.15/1c.17 items land.

pub mod build;
pub(crate) mod drafts;
mod editor;
pub mod inline;
pub(crate) mod intent;
mod ops;
pub mod persist;
pub mod preview;
pub(crate) mod recall;
mod render;
pub mod reply;
pub(crate) mod style;
pub mod token;

pub use editor::EditorCommand;
pub use inline::InlineEditor;
pub use ops::{dispatch, scroll, start_compose, start_compose_to};
pub use persist::recover;
pub use preview::{AttachPreview, PreviewPlugin};
pub use recall::recall_selected;
pub use render::ComposeWidget;
pub use reply::{ReplyKind, start_reply};

use std::path::PathBuf;

use bevy::prelude::*;
use nitidus_mail::AccountId;

use crate::config::Config;
use crate::config::account::AccountConfig;

const COMPOSE_DIR_NAME: &str = "compose";

/// Where compose bodies live. A resource so tests (and later, config)
/// can redirect it; defaults to `state_dir/compose`.
#[derive(Resource)]
pub struct ComposeDir(pub PathBuf);
const SIGNATURE_SEPARATOR: &str = "-- ";

pub struct ComposePlugin;

impl Plugin for ComposePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComposeState>();
        app.init_resource::<InlineEditor>();
        app.add_plugins(preview::PreviewPlugin);
        app.init_resource::<intent::ReplyIntent>();
        app.add_systems(Startup, (render::spawn_compose, persist::notice_orphans));
        app.add_systems(
            Update,
            (
                intent::consume_reply_intent,
                sync_attachments,
                render::apply_placement,
                render::refresh_compose,
                persist::persist_session,
            )
                .chain(),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposeStage {
    Prompting,
    Editing,
    Review,
}

pub struct ComposeSession {
    pub account: AccountId,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body_path: PathBuf,
    pub body: Vec<String>,
    pub stage: ComposeStage,
    /// Threading headers when this is a reply.
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// The message being answered — gains `\Answered` once the reply
    /// actually sends.
    pub reply_source: Option<ReplySource>,
    pub attachments: Vec<std::path::PathBuf>,
    /// The server draft this session was recalled from; replaced on
    /// the next postpone or send.
    pub draft_source: Option<(nitidus_mail::FolderId, nitidus_mail::EnvelopeId)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplySource {
    pub account: AccountId,
    pub folder: nitidus_mail::FolderId,
    pub id: nitidus_mail::EnvelopeId,
}

#[derive(Resource, Default)]
pub struct ComposeState(pub(crate) Option<ComposeSession>);

impl ComposeState {
    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }

    pub fn session(&self) -> Option<&ComposeSession> {
        self.0.as_ref()
    }
}

impl ComposeSession {
    /// Creates the session and its body file: `initial_content` (a
    /// reply quote, or empty) first, signature after.
    fn create(
        account_config: &AccountConfig,
        directory: &std::path::Path,
        initial_content: &str,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(directory)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or_default();
        let body_path = directory.join(format!("{stamp}-{}.md", std::process::id()));
        let body = format!("{initial_content}{}", initial_body(account_config));
        std::fs::write(&body_path, &body)?;
        Ok(Self {
            account: AccountId::new(&account_config.name),
            from: from_identity(account_config),
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            subject: String::new(),
            body_path,
            body: body.lines().map(str::to_owned).collect(),
            stage: ComposeStage::Prompting,
            in_reply_to: None,
            references: Vec::new(),
            reply_source: None,
            attachments: Vec::new(),
            draft_source: None,
        })
    }

    pub(crate) fn reload_body(&mut self) {
        self.body = std::fs::read_to_string(&self.body_path)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
    }

    /// `body_path` is what the send and postpone paths read, so any edit
    /// to `body` outside the editor has to reach the file too.
    pub(crate) fn write_body(&self) -> std::io::Result<()> {
        std::fs::write(&self.body_path, format!("{}\n", self.body.join("\n")))
    }
}

/// The body names the attachments; `session.attachments` is a cache of
/// what its tokens say. Keeping it derived means `build`, `persist`, and
/// the outbox go on reading one field and never learn about tokens.
///
/// Reads go through `Deref` so only a real change ticks the resource,
/// which would otherwise redraw the composer every frame.
fn sync_attachments(mut compose: ResMut<ComposeState>) {
    if !compose.is_changed() {
        return;
    }
    let Some(derived) = compose.session().map(|session| token::paths(&session.body)) else {
        return;
    };
    if compose
        .session()
        .is_some_and(|session| session.attachments == derived)
    {
        return;
    }
    if let Some(session) = compose.0.as_mut() {
        session.attachments = derived;
    }
}

fn from_identity(account: &AccountConfig) -> String {
    if account.display_name.is_empty() {
        account.email.clone()
    } else {
        format!("{} <{}>", account.display_name, account.email)
    }
}

fn initial_body(account: &AccountConfig) -> String {
    let signature = account.signature.clone().or_else(|| {
        account
            .signature_file
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
    });
    match signature {
        Some(signature) => format!("\n\n{SIGNATURE_SEPARATOR}\n{}\n", signature.trim_end()),
        None => String::new(),
    }
}

pub(crate) fn compose_directory(world: &World) -> anyhow::Result<PathBuf> {
    match world.get_resource::<ComposeDir>() {
        Some(directory) => Ok(directory.0.clone()),
        None => Ok(crate::dirs::state_dir()?.join(COMPOSE_DIR_NAME)),
    }
}

/// The account whose identity a new message uses: the viewed account,
/// else the first configured one.
pub(crate) fn composing_account(world: &World) -> Option<AccountConfig> {
    let config = world.resource::<Config>();
    let viewed = world
        .get_resource::<crate::index::IndexView>()
        .and_then(|view| view.account.clone());
    match viewed {
        Some(account) => config
            .accounts
            .iter()
            .find(|candidate| candidate.name == account.as_str())
            .cloned(),
        None => config.accounts.first().cloned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn from_identity_prefers_display_name() {
        let mut account = AccountConfig {
            email: "n@x.com".to_owned(),
            ..Default::default()
        };
        assert_eq!(from_identity(&account), "n@x.com");
        account.display_name = "Norman".to_owned();
        assert_eq!(from_identity(&account), "Norman <n@x.com>");
    }

    #[test]
    fn initial_body_appends_signature_after_the_marker() {
        let account = AccountConfig {
            signature: Some("Norman\nnitidus".to_owned()),
            ..Default::default()
        };
        assert_eq!(initial_body(&account), "\n\n-- \nNorman\nnitidus\n");
        assert_eq!(initial_body(&AccountConfig::default()), "");
    }

    #[test]
    fn signature_file_is_the_fallback() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "from file\n").unwrap();
        let account = AccountConfig {
            signature_file: Some(file.path().to_path_buf()),
            ..Default::default()
        };
        assert_eq!(initial_body(&account), "\n\n-- \nfrom file\n");
    }
}
