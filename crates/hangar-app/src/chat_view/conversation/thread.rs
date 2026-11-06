//! The conversation as the window holds it between reads: the items read so
//! far, the newest page merged into them, the older pages put in front, and
//! the messages the person sent that the transcript does not have yet.
//!
//! Only the newest page is read again, so the pages scrolled back to stay as
//! they were: the newest page is laid over the end of what is held, from
//! its first item on. When that item is not held — more was written since
//! than a page holds, or the conversation started over — the page replaces
//! everything, with its own cursor.

use hangar_core::chat::{Body, ChatItem, Sender};

use crate::mobile::contract::{ChatPending, ChatReason};
use crate::mobile::desk::ChatView;

/// How long a message the person sent waits to be found in the transcript
/// before it is no longer shown apart: by then it is in, under other words
/// (Claude Code rewrites a pasted image as its mark), or it never will be.
pub const OUTGOING_KEPT_MS: i64 = 120_000;
/// A sent message found in the transcript may be stamped a little before the
/// window's clock said it went.
const CLOCK_SLACK_MS: i64 = 5_000;

/// A message the person sent from the chat, shown at once (optimistic) until
/// the transcript has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Outgoing {
    pub text: String,
    /// When it was sent, in ms.
    pub at: i64,
    pub state: Sent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sent {
    /// On its way to the pane.
    Sending,
    /// Typed into the pane.
    Typed,
    /// Held in the queue until the pane's Claude Code is up.
    Queued,
    /// Refused, with the reason.
    Failed(String),
}

/// What the window holds of one pane's conversation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Thread {
    /// Oldest first.
    pub items: Vec<ChatItem>,
    /// There is something before the first item.
    pub older: bool,
    /// What reads the page before the first item.
    pub cursor: Option<String>,
    /// What the pane waits on the person for.
    pub pending: Option<ChatPending>,
    /// Why there is no conversation, when there is none.
    pub reason: Option<ChatReason>,
    /// Sent from the chat, not in the transcript yet; oldest first.
    pub outgoing: Vec<Outgoing>,
}

impl Thread {
    /// The newest page, laid over the end of what is held.
    pub fn take_newest(&mut self, view: ChatView, now: i64) {
        self.pending = view.pending;
        self.reason = view.reason;
        let page = view.items;
        let at = page
            .first()
            .and_then(|first| self.items.iter().position(|item| item.id == first.id));
        match at {
            // Earlier pages are held: they and their cursor stand.
            Some(at) if at > 0 => {
                self.items.truncate(at);
                self.items.extend(page);
            }
            _ => {
                self.items = page;
                self.older = view.older;
                self.cursor = view.cursor;
            }
        }
        self.reconcile(now);
    }

    /// An older page, put in front of what is held.
    pub fn take_older(&mut self, view: ChatView) {
        let mut page: Vec<ChatItem> = view
            .items
            .into_iter()
            .filter(|item| !self.items.iter().any(|held| held.id == item.id))
            .collect();
        page.append(&mut self.items);
        self.items = page;
        self.older = view.older;
        self.cursor = view.cursor;
    }

    /// A message sent from the chat, shown at once.
    pub fn send(&mut self, text: String, now: i64) {
        self.outgoing.push(Outgoing {
            text,
            at: now,
            state: Sent::Sending,
        });
    }

    /// What became of the message sent at `at`.
    pub fn sent(&mut self, at: i64, state: Sent) {
        if let Some(out) = self.outgoing.iter_mut().find(|out| out.at == at) {
            out.state = state;
        }
    }

    /// Drops the messages sent from the chat that the transcript now has,
    /// and those it never will.
    fn reconcile(&mut self, now: i64) {
        let mut found: Vec<&ChatItem> = self
            .items
            .iter()
            .filter(|item| {
                matches!(
                    item.body,
                    Body::User {
                        from: Sender::Person,
                        ..
                    }
                )
            })
            .collect();
        self.outgoing.retain(|out| {
            if matches!(out.state, Sent::Sending) {
                return true;
            }
            let wanted = words(&out.text);
            if let Some(at) = found.iter().position(|item| {
                item.at >= out.at - CLOCK_SLACK_MS
                    && matches!(&item.body, Body::User { text, .. } if words(text) == wanted)
            }) {
                // One transcript line answers for one message sent.
                found.remove(at);
                return false;
            }
            match out.state {
                // A refusal stays until the person sends again; a message
                // held in the queue until it is typed and read.
                Sent::Failed(_) | Sent::Queued => true,
                _ => now - out.at < OUTGOING_KEPT_MS,
            }
        });
    }

    /// Forgets the refusals: the person sent something else.
    pub fn clear_failed(&mut self) {
        self.outgoing
            .retain(|out| !matches!(out.state, Sent::Failed(_)));
    }
}

/// A message's words, whatever spaces and line ends Claude Code kept.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(id: &str, at: i64, text: &str) -> ChatItem {
        ChatItem {
            id: id.into(),
            at,
            body: Body::User {
                text: text.into(),
                from: Sender::Person,
                images: 0,
            },
        }
    }

    fn answered(id: &str) -> ChatItem {
        ChatItem {
            id: id.into(),
            at: 0,
            body: Body::Assistant { text: id.into() },
        }
    }

    fn view(items: Vec<ChatItem>, older: bool, cursor: Option<&str>) -> ChatView {
        ChatView {
            pane_id: "leo".into(),
            pane: "Leo".into(),
            workspace: "Shop".into(),
            agent: "claude".into(),
            activity: None,
            session_id: Some("s".into()),
            items,
            older,
            cursor: cursor.map(str::to_string),
            pending: None,
            reason: None,
        }
    }

    fn ids(thread: &Thread) -> Vec<&str> {
        thread.items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn the_newest_page_is_laid_over_the_end_and_the_older_pages_stay() {
        let mut thread = Thread::default();
        thread.take_newest(
            view(vec![answered("c"), answered("d")], true, Some("c1")),
            0,
        );
        thread.take_older(view(vec![answered("a"), answered("b")], false, None));
        assert_eq!(ids(&thread), ["a", "b", "c", "d"]);
        assert!(!thread.older);

        // Two more lines written: the newest page starts further on.
        thread.take_newest(
            view(
                vec![answered("d"), answered("e"), answered("f")],
                true,
                Some("d1"),
            ),
            0,
        );
        assert_eq!(ids(&thread), ["a", "b", "c", "d", "e", "f"]);
        assert!(!thread.older, "the oldest page's cursor stands");
        assert_eq!(thread.cursor, None);
    }

    #[test]
    fn a_page_that_does_not_meet_what_is_held_replaces_it() {
        let mut thread = Thread::default();
        thread.take_newest(view(vec![answered("a"), answered("b")], false, None), 0);
        thread.take_newest(
            view(vec![answered("x"), answered("y")], true, Some("x1")),
            0,
        );
        assert_eq!(ids(&thread), ["x", "y"]);
        assert!(thread.older);
        assert_eq!(thread.cursor.as_deref(), Some("x1"));
    }

    #[test]
    fn an_older_page_never_repeats_what_is_held() {
        let mut thread = Thread::default();
        thread.take_newest(
            view(vec![answered("b"), answered("c")], true, Some("b1")),
            0,
        );
        thread.take_older(view(vec![answered("a"), answered("b")], false, None));
        assert_eq!(ids(&thread), ["a", "b", "c"]);
    }

    #[test]
    fn a_message_sent_shows_until_the_transcript_has_it() {
        let mut thread = Thread::default();
        thread.send("fix  the\nbuild".into(), 10_000);
        thread.take_newest(view(vec![], false, None), 10_500);
        assert_eq!(thread.outgoing.len(), 1, "still on its way");

        thread.sent(10_000, Sent::Typed);
        thread.take_newest(
            view(vec![said("old", 1_000, "fix the build")], false, None),
            11_000,
        );
        assert_eq!(
            thread.outgoing.len(),
            1,
            "the same words said long before are another message"
        );

        thread.take_newest(
            view(
                vec![
                    said("old", 1_000, "fix the build"),
                    said("new", 9_000, "fix the build"),
                ],
                false,
                None,
            ),
            12_000,
        );
        assert!(
            thread.outgoing.is_empty(),
            "found, within the clock's slack"
        );
    }

    #[test]
    fn a_typed_message_never_found_goes_after_a_while_a_refused_one_stays() {
        let mut thread = Thread::default();
        thread.send("a".into(), 0);
        thread.sent(0, Sent::Typed);
        thread.send("b".into(), 1);
        thread.sent(1, Sent::Failed("no".into()));
        thread.take_newest(view(vec![], false, None), OUTGOING_KEPT_MS + 10);
        assert_eq!(thread.outgoing.len(), 1);
        assert_eq!(thread.outgoing[0].text, "b");
        thread.clear_failed();
        assert!(thread.outgoing.is_empty());
    }
}
