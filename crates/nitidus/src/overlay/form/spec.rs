//! What a caller describes when it opens a form. Purely declarative:
//! behavior lives in `state`, drawing in `render`.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::World;

pub type SubmitFn = Box<dyn FnOnce(&mut World, FormValues) + Send + Sync>;
/// Derives the page list from what has been filled in so far, so a
/// branching flow is data rather than control flow.
pub type PagesFn = Box<dyn Fn(&FormValues) -> Vec<PageSpec> + Send + Sync>;
pub type CancelFn = Arc<dyn Fn(&mut World) -> CancelOutcome + Send + Sync>;

/// What cancelling did. A form that wants to ask first — the composer,
/// which puts a discard confirm in the way — keeps itself open and
/// closes from inside the answer instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelOutcome {
    Close,
    Keep,
}
/// Rejects a value with a message the form shows beside the field.
pub type ValidateFn = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;
/// What Enter on a field does, when stepping forward is not it.
pub type ActivateFn = Arc<dyn Fn(&mut World) + Send + Sync>;
/// Candidates for what has been typed into a field so far.
pub type CompleteFn = Arc<dyn Fn(&str) -> Vec<String> + Send + Sync>;
/// The style each line of a body field is drawn in, or `None` to leave
/// it at the base style.
pub type BodyStyleFn = Arc<
    dyn Fn(&[String], &nitidus_ui_kit::theme::Theme) -> Vec<Option<ratatui::style::Style>>
        + Send
        + Sync,
>;

/// Creating walks the steps in order; editing reaches any of them at
/// once. The two flows want opposite things from the same surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormMode {
    #[default]
    Create,
    Edit,
}

/// Where a form draws. A modal sizes itself and takes the middle of the
/// screen; a hosted form is handed a rect by its host — the composer's
/// reading column — and fills it, chrome pinned to the bottom.
#[derive(Clone, Default)]
pub enum FormPlacement {
    #[default]
    Overlay,
    Host {
        layout: plurimus::LayoutFn,
        order: i32,
    },
}

impl FormPlacement {
    /// The rung the frame draws on; controls take the one above it.
    pub(super) fn order(&self) -> i32 {
        match self {
            Self::Overlay => nitidus_ui_kit::layer::OVERLAY,
            Self::Host { order, .. } => *order,
        }
    }
}

pub struct PageSpec {
    pub id: &'static str,
    pub title: String,
    pub fields: Vec<FieldSpec>,
}

impl PageSpec {
    pub fn new(id: &'static str, title: impl Into<String>, fields: Vec<FieldSpec>) -> Self {
        Self {
            id,
            title: title.into(),
            fields,
        }
    }
}

pub struct FormSpec {
    pub title: String,
    pub mode: FormMode,
    /// The affirmative button's label — "Create", "Save", "Set".
    pub primary_label: String,
    /// The negative button's label. "Cancel" for a form you can walk
    /// away from, "Discard" for one that is throwing something away.
    pub cancel_label: String,
    pub placement: FormPlacement,
    /// A keymap layer beneath the form's own, for a form that belongs to
    /// a larger mode — the composer's commands answer from every field.
    pub context: Option<&'static str>,
    /// Which field takes focus when the form opens. Defaults to the
    /// first, which is wrong for a form whose first field is a label.
    pub initial_focus: Option<&'static str>,
    /// Whether Enter on a *field* fires the primary action. True is the
    /// quick path through a wizard; the composer turns it off, where a
    /// stray Enter in a header would send the message.
    pub enter_activates: bool,
    pub pages: PagesFn,
    pub on_submit: SubmitFn,
    pub on_cancel: CancelFn,
}

impl FormSpec {
    /// A single-page form: no step strip, no Back button.
    pub fn new(
        title: impl Into<String>,
        primary_label: impl Into<String>,
        fields: Vec<FieldSpec>,
        on_submit: SubmitFn,
    ) -> Self {
        let pages: PagesFn =
            Box::new(move |_| vec![PageSpec::new(SINGLE_PAGE_ID, String::new(), fields.clone())]);
        Self::paged(title, primary_label, pages, on_submit)
    }

    pub fn paged(
        title: impl Into<String>,
        primary_label: impl Into<String>,
        pages: PagesFn,
        on_submit: SubmitFn,
    ) -> Self {
        Self {
            title: title.into(),
            mode: FormMode::Create,
            primary_label: primary_label.into(),
            cancel_label: DEFAULT_CANCEL_LABEL.to_owned(),
            placement: FormPlacement::default(),
            context: None,
            initial_focus: None,
            enter_activates: true,
            pages,
            on_submit,
            on_cancel: Arc::new(|_| CancelOutcome::Close),
        }
    }

    pub fn editing(mut self) -> Self {
        self.mode = FormMode::Edit;
        self
    }

    pub fn placed(mut self, placement: FormPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Runs on Esc and on the negative button. Returning `Keep` leaves
    /// the form open — for a confirm that has to answer first.
    pub fn with_cancel(
        mut self,
        on_cancel: impl Fn(&mut World) -> CancelOutcome + Send + Sync + 'static,
    ) -> Self {
        self.on_cancel = Arc::new(on_cancel);
        self
    }

    pub fn cancel_label(mut self, label: impl Into<String>) -> Self {
        self.cancel_label = label.into();
        self
    }

    pub fn in_context(mut self, context: &'static str) -> Self {
        self.context = Some(context);
        self
    }

    pub fn focusing(mut self, id: &'static str) -> Self {
        self.initial_focus = Some(id);
        self
    }

    /// Enter walks the fields instead of firing the primary action.
    pub fn stepping_enter(mut self) -> Self {
        self.enter_activates = false;
        self
    }
}

const DEFAULT_CANCEL_LABEL: &str = "Cancel";

const SINGLE_PAGE_ID: &str = "";

/// How tall a field draws. `Fill` takes whatever the frame has left
/// after the fixed rows and the chrome — a body needs the room, and only
/// the frame knows how much there is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FieldHeight {
    #[default]
    Row,
    Fill,
}

#[derive(Clone)]
pub struct FieldSpec {
    /// Stable key into `FormValues`; survives respawns and page changes.
    pub id: &'static str,
    pub label: String,
    pub kind: FieldKind,
    pub height: FieldHeight,
    /// A field you can reach and read but not change — the composer's
    /// From, which is the account's identity rather than an answer.
    pub read_only: bool,
    pub initial: String,
    pub validate: Option<ValidateFn>,
    pub complete: Option<CompleteFn>,
    pub activate: Option<ActivateFn>,
}

impl FieldSpec {
    pub fn text(id: &'static str, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: FieldKind::Text { masked: false },
            height: FieldHeight::Row,
            read_only: false,
            initial: String::new(),
            validate: None,
            complete: None,
            activate: None,
        }
    }

    /// Reachable by Tab and by the pointer, but it refuses every edit.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Renders the value as `*` per character (secrets).
    pub fn masked(mut self) -> Self {
        self.kind = FieldKind::Text { masked: true };
        self
    }

    /// Takes the rows the frame has left over. At most one field per
    /// page can, and a second one splits the remainder with the first.
    pub fn filling(mut self) -> Self {
        self.height = FieldHeight::Fill;
        self
    }

    pub fn with_initial(mut self, initial: impl Into<String>) -> Self {
        self.initial = initial.into();
        self
    }

    pub fn validated(
        mut self,
        validate: impl Fn(&str) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.validate = Some(Arc::new(validate));
        self
    }

    /// Offers candidates for the segment being typed. Unlike a select,
    /// the field still accepts anything — the candidates are a
    /// shortcut, not the vocabulary.
    pub fn completed(
        mut self,
        complete: impl Fn(&str) -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        self.complete = Some(Arc::new(complete));
        self
    }

    /// A row of entries — attachments, say — stepped through with Left
    /// and Right, and removed with Delete. `empty_label` is what it
    /// offers when there is nothing in it yet.
    pub fn entries(
        id: &'static str,
        label: impl Into<String>,
        empty_label: impl Into<String>,
    ) -> Self {
        Self {
            id,
            label: label.into(),
            kind: FieldKind::Entries {
                empty_label: empty_label.into(),
            },
            height: FieldHeight::Row,
            read_only: false,
            initial: String::new(),
            validate: None,
            complete: None,
            activate: None,
        }
    }

    /// What Enter does on this field instead of stepping forward.
    pub fn activated(mut self, activate: impl Fn(&mut World) + Send + Sync + 'static) -> Self {
        self.activate = Some(Arc::new(activate));
        self
    }

    /// A field holding many lines rather than one. It fills the frame
    /// by default — a body is the reason a form needs the room.
    pub fn body(id: &'static str, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: FieldKind::Body { style: None },
            height: FieldHeight::Fill,
            read_only: false,
            initial: String::new(),
            validate: None,
            complete: None,
            activate: None,
        }
    }

    /// Colours the body's lines — quoted text and signatures dimmed,
    /// say. Ignored by every other kind of field.
    pub fn line_styled(
        mut self,
        style: impl Fn(&[String], &nitidus_ui_kit::theme::Theme) -> Vec<Option<ratatui::style::Style>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.kind = FieldKind::Body {
            style: Some(Arc::new(style)),
        };
        self
    }

    /// A field that cycles a fixed set of options instead of accepting
    /// text.
    pub fn select(id: &'static str, label: impl Into<String>, options: Vec<SelectOption>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: FieldKind::Select { options },
            height: FieldHeight::Row,
            read_only: false,
            initial: String::new(),
            validate: None,
            complete: None,
            activate: None,
        }
    }

    pub fn is_masked(&self) -> bool {
        matches!(self.kind, FieldKind::Text { masked: true })
    }

    pub fn options(&self) -> &[SelectOption] {
        match &self.kind {
            FieldKind::Select { options } => options,
            _ => &[],
        }
    }

    pub fn is_select(&self) -> bool {
        matches!(self.kind, FieldKind::Select { .. })
    }

    pub fn is_body(&self) -> bool {
        matches!(self.kind, FieldKind::Body { .. })
    }

    pub fn is_entries(&self) -> bool {
        matches!(self.kind, FieldKind::Entries { .. })
    }

    pub(super) fn empty_label(&self) -> &str {
        match &self.kind {
            FieldKind::Entries { empty_label } => empty_label,
            _ => "",
        }
    }

    pub(super) fn body_style(&self) -> Option<&BodyStyleFn> {
        match &self.kind {
            FieldKind::Body { style } => style.as_ref(),
            _ => None,
        }
    }

    /// What this field holds before anyone touches it. A select always
    /// holds one of its options, so its answer is well-defined from the
    /// moment the form opens rather than from the first keystroke.
    pub fn resolved_initial(&self) -> String {
        if !self.is_select() {
            return self.initial.clone();
        }
        let options = self.options();
        options
            .iter()
            .find(|option| option.value == self.initial)
            .or_else(|| options.first())
            .map(|option| option.value.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone)]
pub enum FieldKind {
    Text {
        masked: bool,
    },
    Select {
        options: Vec<SelectOption>,
    },
    Body {
        style: Option<BodyStyleFn>,
    },
    /// A row of entries you step through rather than type into. Its
    /// value is the entries, one per line.
    Entries {
        empty_label: String,
    },
}

/// One choice in a select field. `value` is what lands in `FormValues`;
/// `label` is what the user reads and `detail` explains it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// The values a form collects, keyed by `FieldSpec::id`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormValues(HashMap<&'static str, String>);

impl FormValues {
    pub fn get(&self, id: &str) -> &str {
        self.0.get(id).map_or("", String::as_str)
    }

    pub fn set(&mut self, id: &'static str, value: impl Into<String>) {
        self.0.insert(id, value.into());
    }

    /// Distinguishes "never filled in" from "deliberately cleared", so a
    /// spec's initial value applies only until the form has an answer.
    pub fn contains(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn missing_values_read_as_empty() {
        let mut values = FormValues::default();
        assert_eq!(values.get("absent"), "");
        values.set("name", "work");
        assert_eq!(values.get("name"), "work");
    }

    #[test]
    fn select_fields_carry_their_options_and_text_fields_do_not() {
        let field = FieldSpec::select(
            "provider",
            "Provider",
            vec![SelectOption::new("gmail", "Gmail").with_detail("imap.gmail.com")],
        );
        assert!(field.is_select());
        assert_eq!(field.options().len(), 1);
        assert_eq!(field.options()[0].detail.as_deref(), Some("imap.gmail.com"));
        assert!(FieldSpec::text("name", "Name").options().is_empty());
    }

    #[test]
    fn text_fields_are_unmasked_until_asked() {
        let field = FieldSpec::text("password", "Password");
        assert!(!field.is_masked());
        assert!(FieldSpec::text("password", "Password").masked().is_masked());
    }
}
