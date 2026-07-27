//! What a caller describes when it opens a form. Purely declarative:
//! behavior lives in `state`, drawing in `render`.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::World;

pub type SubmitFn = Box<dyn FnOnce(&mut World, FormValues) + Send + Sync>;
/// Derives the page list from what has been filled in so far, so a
/// branching flow is data rather than control flow.
pub type PagesFn = Box<dyn Fn(&FormValues) -> Vec<PageSpec> + Send + Sync>;
pub type CancelFn = Box<dyn FnOnce(&mut World) + Send + Sync>;
/// Rejects a value with a message the form shows beside the field.
pub type ValidateFn = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;
/// Candidates for what has been typed into a field so far.
pub type CompleteFn = Arc<dyn Fn(&str) -> Vec<String> + Send + Sync>;

/// Creating walks the steps in order; editing reaches any of them at
/// once. The two flows want opposite things from the same surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormMode {
    #[default]
    Create,
    Edit,
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
            pages,
            on_submit,
            on_cancel: Box::new(|_| {}),
        }
    }

    pub fn editing(mut self) -> Self {
        self.mode = FormMode::Edit;
        self
    }

    pub fn with_cancel(mut self, on_cancel: CancelFn) -> Self {
        self.on_cancel = on_cancel;
        self
    }
}

const SINGLE_PAGE_ID: &str = "";

#[derive(Clone)]
pub struct FieldSpec {
    /// Stable key into `FormValues`; survives respawns and page changes.
    pub id: &'static str,
    pub label: String,
    pub kind: FieldKind,
    pub initial: String,
    pub validate: Option<ValidateFn>,
    pub complete: Option<CompleteFn>,
}

impl FieldSpec {
    pub fn text(id: &'static str, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: FieldKind::Text { masked: false },
            initial: String::new(),
            validate: None,
            complete: None,
        }
    }

    /// Renders the value as `*` per character (secrets).
    pub fn masked(mut self) -> Self {
        self.kind = FieldKind::Text { masked: true };
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

    /// A field that cycles a fixed set of options instead of accepting
    /// text.
    pub fn select(id: &'static str, label: impl Into<String>, options: Vec<SelectOption>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: FieldKind::Select { options },
            initial: String::new(),
            validate: None,
            complete: None,
        }
    }

    pub fn is_masked(&self) -> bool {
        matches!(self.kind, FieldKind::Text { masked: true })
    }

    pub fn options(&self) -> &[SelectOption] {
        match &self.kind {
            FieldKind::Text { .. } => &[],
            FieldKind::Select { options } => options,
        }
    }

    pub fn is_select(&self) -> bool {
        matches!(self.kind, FieldKind::Select { .. })
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
    Text { masked: bool },
    Select { options: Vec<SelectOption> },
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
