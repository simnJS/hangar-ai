//! The two commands run around a workspace's worktrees
//! (`WorkspaceHooksSection` and `cleanHooks` of WorkspaceDialog.tsx).
//!
//! Typed by the person and by nobody else: they run on this machine, as them.
//! A blank field runs nothing, and is stored as absent.

use hangar_i18n::Key;
use hangar_theme::tokens;
use hangar_ui::text as typography;
use iced::widget::{column, text_input};
use iced::{Element, Padding};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::types::WorktreeHooks;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookField {
    PostCreate,
    PreRemove,
}

/// The fields as typed: what the person sees, before blanks are dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HooksDraft {
    pub post_create: String,
    pub pre_remove: String,
}

impl HooksDraft {
    pub fn of(hooks: Option<&WorktreeHooks>) -> HooksDraft {
        HooksDraft {
            post_create: hooks
                .and_then(|hooks| hooks.post_create.clone())
                .unwrap_or_default(),
            pre_remove: hooks
                .and_then(|hooks| hooks.pre_remove.clone())
                .unwrap_or_default(),
        }
    }

    pub fn set(&mut self, field: HookField, text: String) {
        match field {
            HookField::PostCreate => self.post_create = text,
            HookField::PreRemove => self.pre_remove = text,
        }
    }

    pub fn get(&self, field: HookField) -> &str {
        match field {
            HookField::PostCreate => &self.post_create,
            HookField::PreRemove => &self.pre_remove,
        }
    }

    /// Blank commands are dropped; nothing left means no setting at all.
    pub fn clean(&self) -> Option<WorktreeHooks> {
        let post_create = self.post_create.trim();
        let pre_remove = self.pre_remove.trim();
        if post_create.is_empty() && pre_remove.is_empty() {
            return None;
        }
        Some(WorktreeHooks {
            post_create: (!post_create.is_empty()).then(|| post_create.to_string()),
            pre_remove: (!pre_remove.is_empty()).then(|| pre_remove.to_string()),
        })
    }
}

/// The section: what runs where, then the two fields with their hints.
/// `message` makes the message that edits a field (the creation dialog's or
/// the kits dialog's). `submit` is what Return in a field says: the creation
/// dialog is a form that Return sends, the kits dialog saves as it goes and
/// has nothing to send.
pub fn view<'a>(
    ctx: Ctx<'a>,
    draft: &'a HooksDraft,
    submit: Option<Message>,
    message: impl Fn(HookField, String) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let placeholder = placeholder(ctx);
    let field = |field: HookField, label: Key, hint: Key| -> Element<'a, Message> {
        column![
            typography::hint(t, ctx.t(label)),
            crate::keymap::guard(
                ctx,
                text_input(placeholder, draft.get(field))
                    .size(tokens::font_size::UI)
                    .padding(Padding::from([5.0, tokens::space::MD]))
                    .font(hangar_theme::fonts::mono())
                    .on_input(move |text| message(field, text))
                    .on_submit_maybe(submit.clone())
                    .style(hangar_theme::style::text_input::field(t)),
            ),
            typography::hint(t, ctx.t(hint)),
        ]
        .spacing(tokens::space::XS)
        .into()
    };
    column![
        typography::hint(t, ctx.t(Key::WorktreesHooksRunsHere)),
        field(
            HookField::PostCreate,
            Key::WorktreesHooksPostCreate,
            Key::WorktreesHooksPostCreateHint
        ),
        field(
            HookField::PreRemove,
            Key::WorktreesHooksPreRemove,
            Key::WorktreesHooksPreRemoveHint
        ),
    ]
    .spacing(tokens::space::LG)
    .into()
}

/// A dictionary string without arguments is borrowed from the dictionary,
/// which is what a field's placeholder needs to outlive the view.
fn placeholder(ctx: Ctx<'_>) -> &'static str {
    match ctx.t(Key::WorktreesHooksPlaceholder) {
        std::borrow::Cow::Borrowed(text) => text,
        std::borrow::Cow::Owned(_) => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(post_create: &str, pre_remove: &str) -> HooksDraft {
        HooksDraft {
            post_create: post_create.into(),
            pre_remove: pre_remove.into(),
        }
    }

    #[test]
    fn blank_commands_are_dropped_and_nothing_left_is_no_setting() {
        assert_eq!(draft("", "").clean(), None);
        assert_eq!(draft("  ", "\t").clean(), None);
        assert_eq!(
            draft(" npm install ", "").clean(),
            Some(WorktreeHooks {
                post_create: Some("npm install".into()),
                pre_remove: None,
            })
        );
        assert_eq!(
            draft("a", " b ").clean(),
            Some(WorktreeHooks {
                post_create: Some("a".into()),
                pre_remove: Some("b".into()),
            })
        );
    }

    #[test]
    fn a_workspaces_hooks_fill_the_fields_and_come_back_cleaned() {
        let hooks = WorktreeHooks {
            post_create: Some("pnpm i".into()),
            pre_remove: None,
        };
        let mut fields = HooksDraft::of(Some(&hooks));
        assert_eq!(fields.get(HookField::PostCreate), "pnpm i");
        assert_eq!(fields.get(HookField::PreRemove), "");
        // Typing a space after a word keeps it in the field, where the web
        // app's live-cleaned value would have swallowed it.
        fields.set(HookField::PostCreate, "pnpm i ".into());
        assert_eq!(fields.get(HookField::PostCreate), "pnpm i ");
        assert_eq!(fields.clean(), Some(hooks));
        assert_eq!(HooksDraft::of(None), HooksDraft::default());
    }
}
