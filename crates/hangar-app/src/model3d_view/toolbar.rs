//! The bar over the viewport (`ModelToolbar`): recentre, the display toggles,
//! and the capture menu.

use iced::Element;

use hangar_i18n::Key;
use hangar_model3d::DisplayOptions;
use hangar_ui::{Icon, menu};

use super::capture::Destination;
use super::{Msg, Toggle};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::preview::parts::{self, Notice};

/// What the bar needs to know about the model in front.
pub struct Bar<'a> {
    pub id: &'a str,
    /// A model is in memory: without one, every tool is disabled.
    pub ready: bool,
    pub display: DisplayOptions,
    /// The model's normals can be drawn (not past the crate's vertex limit).
    pub normals_allowed: bool,
    /// The capture menu is open.
    pub menu: bool,
    /// A word after the last capture.
    pub notice: Option<&'a Notice>,
}

fn to(msg: Msg) -> Message {
    Message::Model3d(msg)
}

pub fn view<'a>(ctx: Ctx<'a>, bar: Bar<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let (id, ready) = (bar.id, bar.ready);

    // A toggle of the display: pressed when on, disabled with no model.
    let toggle = |which: Toggle, icon: Icon, label: String, on: bool, enabled: bool| {
        parts::icon_tool(
            t,
            icon,
            label,
            Some(on && enabled),
            (ready && enabled).then(|| to(Msg::Toggle(which))),
        )
    };
    // The tooltip of the normals says why they are off when there are too many.
    let normals_label = if bar.normals_allowed || !ready {
        ctx.t(Key::ModelNormals)
    } else {
        ctx.t(Key::ModelNormalsTooMany)
    };

    let tools = vec![
        parts::icon_tool(
            t,
            Icon::Frame,
            ctx.t(Key::ModelRecenter),
            None,
            ready.then(|| to(Msg::Recenter(id.to_string()))),
        ),
        parts::separator(t),
        toggle(
            Toggle::Wireframe,
            Icon::Wireframe,
            ctx.t(Key::ModelWireframe).into_owned(),
            bar.display.wireframe,
            true,
        ),
        toggle(
            Toggle::Grid,
            Icon::Grid,
            ctx.t(Key::ModelGrid).into_owned(),
            bar.display.grid,
            true,
        ),
        toggle(
            Toggle::Axes,
            Icon::Axes,
            ctx.t(Key::ModelAxes).into_owned(),
            bar.display.axes,
            true,
        ),
        toggle(
            Toggle::Normals,
            Icon::Normals,
            normals_label.into_owned(),
            bar.display.normals,
            bar.normals_allowed,
        ),
    ];

    let mut aside: Vec<Element<'a, Message>> = Vec::new();
    if let Some(notice) = bar.notice {
        aside.push(parts::notice_text(t, notice));
    }
    let button = parts::icon_tool(
        t,
        Icon::Camera,
        ctx.t(Key::ModelCapture),
        Some(bar.menu),
        ready.then(|| to(Msg::CaptureMenu(id.to_string(), !bar.menu))),
    );
    let entries = bar.menu.then(|| {
        vec![
            menu::item(ctx.t(Key::ModelCopyImage))
                .icon(Icon::Copy)
                .on_select(to(Msg::Capture(id.to_string(), Destination::Clipboard)))
                .into(),
            menu::item(ctx.t(Key::ModelSaveImage))
                .icon(Icon::Download)
                .on_select(to(Msg::Capture(id.to_string(), Destination::Dialog)))
                .into(),
        ]
    });
    aside.push(
        menu::dropdown(t, button, entries)
            .on_dismiss(to(Msg::CaptureMenu(id.to_string(), false)))
            .into(),
    );
    parts::bar(t, tools, aside)
}
