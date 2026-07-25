//! The command vocabulary: every operation is a named command string
//! that parses to an `Action`. Keybindings, the command line, and future
//! macros all share this one parser.

pub use crate::command::{complete_command, parse_command};
use crate::index::{self, SortMode};
use crate::keymap::{InputMode, Mode};
use crate::status::StatusMessage;
use bevy::app::AppExit;
use bevy::prelude::*;
use nitidus_mail::Flags;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Non-empty text pre-fills the command line (a trailing space is
    /// appended so arguments type straight in).
    OpenCommandLine(String),
    TabNext,
    TabPrev,
    Contacts,
    ContactsFocus,
    ContactEdit,
    ContactEditRaw,
    ContactAdd,
    ContactRemoveProperty,
    NewContact,
    DeleteContact,
    ImportContacts(Option<String>),
    ExportContacts(Option<String>),
    SetPhoto(Option<String>),
    AddContact,
    ComposeTo,
    Echo(String),
    Cursor(Motion),
    Sort(SortMode),
    Flag {
        flag: Flags,
        op: FlagOp,
    },
    ToggleThreads,
    Fold(FoldOp),
    OverlayConfirm,
    OverlayCancel,
    View,
    Pager(PagerOp),
    Sidebar(SidebarOp),
    FolderCreate(String),
    FolderRename(String),
    FolderDelete,
    Help,
    HelpScope,
    Compose,
    ComposeAction(ComposeOp),
    UndoSend,
    Reply(crate::compose::ReplyKind),
    Recall,
    Recover,
    SetPassword,
    DeletePassword,
    Authorize,
    Deauthorize,
    NewAccount,
    EditAccount,
    RemoveAccount,
    Delete,
    Move(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposeOp {
    EditBody,
    To,
    Cc,
    Bcc,
    Subject,
    Send,
    Postpone,
    Discard,
    Attach,
    Detach,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarOp {
    ToggleVisible,
    ToggleFocus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagerOp {
    Close,
    NextMessage,
    PrevMessage,
    ToggleHeaders,
    SkipQuoted,
    NextPart,
    PrevPart,
    SavePart,
    OpenPart,
    Links,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Next,
    Prev,
    NextPage,
    PrevPage,
    First,
    Last,
    Parent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlagOp {
    Set,
    Clear,
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldOp {
    Toggle,
    CollapseAll,
    ExpandAll,
}

/// Applies an action immediately. Direct world mutation (rather than a
/// message hop) keeps mode switches synchronous for burst input.
pub fn apply_action(world: &mut World, action: &Action) {
    match action {
        Action::Quit => {
            world.write_message(AppExit::Success);
        }
        Action::OpenCommandLine(prefill) => {
            world.resource_mut::<Mode>().0 = InputMode::CommandLine;
            world
                .resource_mut::<crate::cmdline::CommandLineState>()
                .prefill(prefill);
        }
        Action::TabNext => crate::shell::switch_tab(world, 1),
        Action::TabPrev => crate::shell::switch_tab(world, -1),
        Action::Contacts => crate::shell::activate_tab(world, crate::shell::CONTACTS_TAB),
        Action::ContactsFocus => crate::contacts::toggle_focus(world),
        Action::ContactEdit => crate::contacts::edit_selected(world),
        Action::ContactEditRaw => crate::contacts::edit_selected_raw(world),
        Action::ContactAdd => crate::contacts::add_property(world),
        Action::ContactRemoveProperty => crate::contacts::remove_selected_property(world),
        Action::NewContact => crate::contacts::new_contact(world),
        Action::DeleteContact => crate::contacts::delete_selected_contact(world),
        Action::ImportContacts(path) => crate::contacts::import_contacts(world, path.as_deref()),
        Action::ExportContacts(path) => crate::contacts::export_contacts(world, path.as_deref()),
        Action::SetPhoto(path) => crate::contacts::set_photo(world, path.as_deref()),
        Action::AddContact => crate::contacts::add_contact_from_sender(world),
        Action::ComposeTo => crate::contacts::compose_to_selected(world),
        Action::Echo(text) => {
            let now = world.resource::<Time>().elapsed_secs_f64();
            world
                .resource_mut::<StatusMessage>()
                .info(text.clone(), now);
        }
        Action::Cursor(motion) => dispatch_motion(world, *motion),
        Action::Sort(mode) => index::set_sort(world, *mode),
        Action::Flag { flag, op } => index::flag_selected(world, *flag, *op),
        Action::ToggleThreads => index::toggle_threads(world),
        Action::Fold(op) => {
            if crate::sidebar::is_focused(world) {
                crate::sidebar::fold(world, *op);
            } else {
                index::fold(world, *op);
            }
        }
        Action::OverlayConfirm => crate::overlay::confirm(world),
        Action::OverlayCancel => crate::overlay::close(world),
        Action::View => {
            if crate::sidebar::is_focused(world) {
                crate::sidebar::select(world);
            } else {
                crate::pager::open_selected(world);
            }
        }
        Action::Pager(op) => crate::pager::dispatch(world, *op),
        Action::Sidebar(SidebarOp::ToggleVisible) => crate::sidebar::toggle_visible(world),
        Action::Sidebar(SidebarOp::ToggleFocus) => crate::sidebar::toggle_focus(world),
        Action::FolderCreate(name) => crate::sidebar::folder_create(world, name),
        Action::FolderRename(new_name) => crate::sidebar::folder_rename(world, new_name),
        Action::FolderDelete => crate::sidebar::folder_delete(world),
        Action::Help => crate::help::open(world, crate::help::HelpScope::Current),
        Action::HelpScope => crate::help::toggle_scope(world),
        Action::Compose => crate::compose::start_compose(world),
        Action::ComposeAction(op) => crate::compose::dispatch(world, *op),
        Action::UndoSend => crate::outbox::undo_send(world),
        Action::Reply(kind) => crate::compose::start_reply(world, *kind),
        Action::Recall => crate::compose::recall_selected(world),
        Action::Recover => crate::compose::recover(world),
        Action::SetPassword => crate::accounts::set_password(world),
        Action::DeletePassword => crate::accounts::delete_password(world),
        Action::Authorize => crate::accounts::oauth::authorize(world),
        Action::Deauthorize => crate::accounts::deauthorize(world),
        Action::NewAccount => crate::accounts::wizard::start(world),
        Action::EditAccount => crate::accounts::manage::edit_account(world),
        Action::RemoveAccount => crate::accounts::manage::remove_account(world),
        Action::Delete => crate::index::delete_selected(world),
        Action::Move(folder) => crate::index::move_selected(world, folder),
    }
}

/// One motion vocabulary, four surfaces: the open overlay wins, then
/// the focused sidebar, then the active screen.
fn dispatch_motion(world: &mut World, motion: Motion) {
    let overlay_open = world
        .get_resource::<crate::overlay::ActiveOverlay>()
        .is_some_and(crate::overlay::ActiveOverlay::is_open);
    if overlay_open {
        return crate::overlay::move_selection(world, motion);
    }
    if crate::sidebar::is_focused(world) {
        return crate::sidebar::move_cursor(world, motion);
    }
    let screen = world
        .get_resource::<crate::screen::Screen>()
        .copied()
        .unwrap_or_default();
    match screen {
        crate::screen::Screen::Pager => crate::pager::scroll(world, motion),
        crate::screen::Screen::Compose => crate::compose::scroll(world, motion),
        crate::screen::Screen::Index => index::move_cursor(world, motion),
        crate::screen::Screen::Contacts => crate::contacts::move_cursor(world, motion),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_known_commands_with_and_without_colon() {
        assert_eq!(parse_command(":quit").unwrap(), Action::Quit);
        assert_eq!(parse_command("quit").unwrap(), Action::Quit);
        assert_eq!(parse_command(":q").unwrap(), Action::Quit);
        assert_eq!(parse_command(":tab-next").unwrap(), Action::TabNext);
        assert_eq!(parse_command(":tab-prev").unwrap(), Action::TabPrev);
        assert_eq!(
            parse_command(":command-line").unwrap(),
            Action::OpenCommandLine(String::new())
        );
    }

    #[test]
    fn echo_keeps_its_arguments() {
        assert_eq!(
            parse_command(":echo hello world").unwrap(),
            Action::Echo("hello world".to_owned())
        );
        assert_eq!(parse_command(":echo").unwrap(), Action::Echo(String::new()));
    }

    #[test]
    fn parses_cursor_sort_and_flag_commands() {
        use crate::index::{SortKey, SortMode};
        assert_eq!(
            parse_command(":next").unwrap(),
            Action::Cursor(Motion::Next)
        );
        assert_eq!(
            parse_command(":prev-page").unwrap(),
            Action::Cursor(Motion::PrevPage)
        );
        assert_eq!(
            parse_command(":last").unwrap(),
            Action::Cursor(Motion::Last)
        );
        assert_eq!(
            parse_command(":sort from -r").unwrap(),
            Action::Sort(SortMode {
                key: SortKey::From,
                reverse: true
            })
        );
        assert_eq!(
            parse_command(":toggle-read").unwrap(),
            Action::Flag {
                flag: Flags::SEEN,
                op: FlagOp::Toggle
            }
        );
        assert_eq!(
            parse_command(":unflag").unwrap(),
            Action::Flag {
                flag: Flags::FLAGGED,
                op: FlagOp::Clear
            }
        );
        assert!(parse_command(":sort sideways").is_err());
    }

    #[test]
    fn parses_sidebar_and_folder_commands() {
        assert_eq!(
            parse_command(":sidebar").unwrap(),
            Action::Sidebar(SidebarOp::ToggleVisible)
        );
        assert_eq!(
            parse_command(":sidebar-focus").unwrap(),
            Action::Sidebar(SidebarOp::ToggleFocus)
        );
        assert_eq!(
            parse_command(":folder-create Archive/2026").unwrap(),
            Action::FolderCreate("Archive/2026".to_owned())
        );
        assert_eq!(
            parse_command(":folder-rename Notes").unwrap(),
            Action::FolderRename("Notes".to_owned())
        );
        assert_eq!(
            parse_command(":folder-delete").unwrap(),
            Action::FolderDelete
        );
        assert!(parse_command(":folder-create").is_err(), "name is required");
        assert!(parse_command(":folder-rename").is_err(), "name is required");
        assert!(parse_command(":folder-delete extra").is_err());
    }

    #[test]
    fn command_line_carries_a_prefill() {
        assert_eq!(
            parse_command(":command-line folder-create").unwrap(),
            Action::OpenCommandLine("folder-create".to_owned())
        );
    }

    #[test]
    fn unknown_and_empty_commands_error_with_context() {
        let message = parse_command(":frobnicate").unwrap_err().to_string();
        assert!(message.contains("frobnicate"), "{message}");
        assert!(parse_command("").is_err());
        assert!(parse_command(":").is_err());
    }

    #[test]
    fn extra_arguments_on_no_arg_commands_error() {
        let message = format!("{:#}", parse_command(":quit now").unwrap_err());
        assert!(message.contains("no arguments"), "{message}");
    }

    #[test]
    fn completion_ranks_fuzzy_matches() {
        let all = complete_command("");
        assert!(all.len() > 30, "all commands list, got {}", all.len());
        assert!(all.contains(&"folder-create".to_owned()));
        let tab = complete_command("tb");
        assert!(tab.contains(&"tab-next".to_owned()), "{tab:?}");
        assert!(complete_command("zzz").is_empty());
    }
}
