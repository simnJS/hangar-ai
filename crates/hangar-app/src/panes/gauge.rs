//! How full the agent's context window is (`PaneGauge.tsx`): a ring and a
//! percentage, sized to survive an 8-pane layout, with the numbers behind
//! them in the tooltip. Amber from 70 %, red from 90 %: the agent compacts on
//! its own near the limit, and these say how near that is. The ring itself
//! is hangar-ui's.

use hangar_i18n::{Key, args};
use iced::Element;

use crate::ctx::Ctx;
use crate::message::Message;
use crate::panes_logic::format_tokens;
use crate::state::live::ContextGauge;
use crate::state::types::AgentId;

/// The gauge of a pane header. `figure` is off in a narrow header, which
/// keeps the ring alone.
pub fn view<'a>(
    ctx: Ctx<'a>,
    gauge: &ContextGauge,
    agent: AgentId,
    figure: bool,
) -> Element<'a, Message> {
    let pct = gauge.pct.round().clamp(0.0, 100.0) as u8;
    let model = gauge
        .model
        .clone()
        .unwrap_or_else(|| agent.as_str().to_owned());
    let words = ctx.i18n.t_with(
        Key::PaneContext,
        &args!(
            "used" => format_tokens(gauge.used_tokens),
            "window" => format_tokens(gauge.window),
            "pct" => u64::from(pct),
            "model" => model,
        ),
    );
    let label = figure.then(|| {
        ctx.i18n
            .t_with(Key::PaneCtxPct, &args!("pct" => u64::from(pct)))
            .into_owned()
    });
    hangar_ui::glyph::gauge(ctx.theme, pct)
        .tip(words.into_owned())
        .label(label)
        .into()
}
