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
    /// A shortcut the tab subscribed to was pressed in it, by its spec.
    Shortcut(String),
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
    /// A menu opened, by what it holds, or the open one closed.
    Menu(Option<u64>),
    /// A menu with an `onOpen` opened: Hangar keeps it open and tells the
    /// kit, which may list other items.
    MenuOpened {
        key: u64,
        view: u32,
        handler: HandlerId,
    },
    /// A long log scrolled: where its view starts, and how tall it is.
    Scrolled { log: String, top: f32, height: f32 },
}
