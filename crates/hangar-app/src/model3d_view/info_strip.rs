//! The model in numbers, under the viewport (`InfoStrip`): its format, the
//! triangles, the size of its bounding box, the materials and the vertices, as
//! an agent gets them back from `model_show`. Items drop out from the least
//! telling when the panel is narrow (the stylesheet's container queries).

use iced::widget::{Space, column, container, responsive, row, rule, text};
use iced::{Alignment, Border, Element, Length, Padding};

use hangar_i18n::{Key, Number};
use hangar_model3d::ModelFormat;
use hangar_theme::{Theme, fonts, style, tokens};
use hangar_ui::tip;

use super::format;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::preview::store::ModelInfo;

/// Below this width the vertices are left out, and below the next the
/// materials too; triangles and the size always stay.
const HIDE_VERTICES_BELOW: f32 = 460.0;
const HIDE_MATERIALS_BELOW: f32 = 360.0;

/// A band under the stage: the chrome's colour, a hairline above it
/// (`inset 0 1px 0 var(--line)`).
pub fn band<'a, M: 'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, M>>,
    height: f32,
    padding: Padding,
) -> Element<'a, M> {
    column![
        rule::horizontal(tokens::BORDER_WIDTH).style(style::rule::hairline(t)),
        container(content)
            .padding(padding)
            .width(Length::Fill)
            .height(height)
            .align_y(Alignment::Center)
            .style(style::container::panel(t)),
    ]
    .into()
}

/// One item of the strip, ready to draw: its value, the words after it, what
/// the tooltip says, and the width below which it is left out.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub value: String,
    pub rest: String,
    pub tip: String,
    pub hide_below: f32,
}

/// Everything the strip shows, as text.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    /// The format's name, as the tab says it (`glb`).
    pub format: String,
    pub items: Vec<Item>,
}

/// The strip's items for a model that has been read, in the order of the web
/// app: triangles, size, materials, vertices.
pub fn facts(ctx: Ctx<'_>, format_name: &str, info: Option<&ModelInfo>) -> Facts {
    let locale = ctx.model.locale();
    let mut items = Vec::new();
    if let Some(info) = info {
        let counted = |value: u64, noun: Key, hide_below: f32| Item {
            value: format::count(value, locale),
            rest: format!(" {}", ctx.i18n.tn(noun, value, &[])),
            tip: Number::from(value).format(locale),
            hide_below,
        };
        items.push(counted(info.triangles, Key::ModelTriangles, 0.0));

        let unit = ModelFormat::from_extension(format_name).and_then(|f| f.plan().unit);
        let size = info
            .size
            .iter()
            .map(|extent| format::length(*extent, locale))
            .collect::<Vec<_>>()
            .join(" × ");
        let explanation = ctx.t(Key::ModelSize);
        items.push(Item {
            value: size,
            rest: unit.map(|unit| format!(" {unit}")).unwrap_or_default(),
            // A format that promises no unit says the numbers are the file's.
            tip: match unit {
                Some(_) => explanation.into_owned(),
                None => format!("{explanation}\n{}", ctx.t(Key::ModelUnitless)),
            },
            hide_below: 0.0,
        });

        items.push(counted(
            info.materials,
            Key::ModelMaterials,
            HIDE_MATERIALS_BELOW,
        ));
        items.push(counted(
            info.vertices,
            Key::ModelVertices,
            HIDE_VERTICES_BELOW,
        ));
    }
    Facts {
        format: format_name.to_string(),
        items,
    }
}

/// The strip. Empty (but as tall) while there is nothing to say.
pub fn view<'a>(ctx: Ctx<'a>, facts: Facts) -> Element<'a, Message> {
    let t = ctx.theme;
    let body = responsive(move |size| {
        let format_chip = container(
            text(facts.format.to_uppercase())
                .size(tokens::font_size::MICRO)
                .font(fonts::mono())
                .style(style::text::muted(t))
                .wrapping(text::Wrapping::None),
        )
        .padding([1.0, 5.0])
        .style(move |_| container::Style {
            border: Border {
                color: t.colors.line_strong,
                width: 1.0,
                radius: tokens::radius::XS.into(),
            },
            ..Default::default()
        });
        let mut line = row![format_chip]
            .spacing(tokens::space::XL)
            .align_y(Alignment::Center);
        for item in facts
            .items
            .iter()
            .filter(|item| size.width >= item.hide_below)
        {
            let words = row![
                text(item.value.clone())
                    .size(tokens::font_size::LABEL)
                    .font(fonts::mono())
                    .style(style::text::secondary(t))
                    .wrapping(text::Wrapping::None),
                text(item.rest.clone())
                    .size(tokens::font_size::LABEL)
                    .font(fonts::mono())
                    .style(style::text::subtle(t))
                    .wrapping(text::Wrapping::None),
            ];
            line = line.push(tip::bottom(t, words, item.tip.clone()));
        }
        line.push(Space::new().width(Length::Fill)).into()
    })
    .width(Length::Fill)
    .height(Length::Fill);
    band(t, body, 26.0, Padding::from([0.0, 10.0]))
}
