//! What a tab's drawing sends back, for the app to hand to
//! [`KitTab::update`](crate::KitTab::update), wrapped in its own message
//! with the tab it came from.

use serde_json::Value;

use hangar_kit_ui::abi::CallId;
use hangar_kit_ui::api::KitError;
use hangar_kit_ui::{HandlerId, Sort, UiEvent};

#[derive(Debug, Clone)]
pub enum KitMessage {
    /// A widget was used: for the kit.
    Ui(UiEvent),
    /// A part of a widget that Hangar keeps itself: a table's order, a
    /// tree's open nodes, the cursor of a text area.
    Local(Local),
    /// A call answered, by the app's task.
    Reply(CallId, Result<Value, KitError>),
    /// A timer of the tab came round.
    Timer(String),
    /// The person asked for the tab again, after a failure or from its bar.
    Reload,
}

#[derive(Debug, Clone)]
pub enum Local {
    /// A sortable table's header pressed, the table sorting by itself.
    Sort { table: String, sort: Sort },
    /// A tree's node opened or closed.
    Toggle {
        tree: String,
        node: String,
        open: bool,
    },
    /// Something done in a text area; an edit also tells the kit the text.
    Edit {
        area: String,
        action: iced::widget::text_editor::Action,
        view: u32,
        handler: Option<HandlerId>,
    },
}
