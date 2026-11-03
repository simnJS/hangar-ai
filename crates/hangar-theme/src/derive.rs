//! The window's colours, derived from a terminal palette and an accent: a
//! port of `deriveChromeTokens` and its helpers in `src/themes.ts`.
//!
//! The bundled themes do not go through this at run time; their tables in
//! `data.rs` were generated from the TypeScript. It is here so that a test can
//! hold the generator and this port to each other, and so that a theme the
//! app does not bundle (a person's own palette, later) restyles the whole
//! window the way a bundled one does, with the same contrast promises.

use iced::Color;

use crate::contrast::{
    HueAndSaturation, Rgb, contrast_ratio, ensure_contrast, hue_and_saturation, hue_distance, mix,
    perceived_lightness, with_hue,
};
use crate::theme::{Appearance, Colors, TerminalColors};

/// Text on the chrome reads at this ratio at least: WCAG's AA for body text.
pub const TEXT_CONTRAST: f64 = 4.5;
/// A state's dot, a bar, a frame: WCAG's 3:1 for what is not text.
pub const FILL_CONTRAST: f64 = 3.0;

/// Two hues closer than this pass for one colour at a glance.
const DISTINCT_HUE: f64 = 30.0;
/// "Your turn" stays somewhere from cyan to blue, whatever it steers clear of.
const YOURS_HUES: (u32, u32) = (165, 240);

/// The colour "your turn" starts from: the theme's cyan, or its blue, as long
/// as it reads as neither the accent (which marks links, focus and the
/// selection) nor working's green. A theme with no such colour has its cyan
/// turned to the nearest hue clear of both.
fn yours_base(accent: Rgb, terminal: &[Rgb; 16], working: Rgb) -> Rgb {
    let accent_hue = hue_and_saturation(accent).hue;
    let working_hue = hue_and_saturation(working).hue;
    let apart = |hue: f64| hue_distance(hue, accent_hue).min(hue_distance(hue, working_hue));
    // The slots of xterm's cyan, bright cyan, blue and bright blue.
    let candidates: Vec<Rgb> = [terminal[6], terminal[14], terminal[4], terminal[12]]
        .into_iter()
        .filter(|&color| {
            // Some palettes fill these slots with a grey, or with what is
            // really a pink: only a cyan or a blue worth the name is a
            // candidate.
            let HueAndSaturation { hue, saturation } = hue_and_saturation(color);
            saturation >= 0.25 && hue >= YOURS_HUES.0 as f64 && hue <= YOURS_HUES.1 as f64
        })
        .collect();
    if let Some(&ready) = candidates
        .iter()
        .find(|&&color| apart(hue_and_saturation(color).hue) >= DISTINCT_HUE)
    {
        return ready;
    }

    let base = candidates.first().copied().unwrap_or(Rgb(0x3f, 0xb8, 0xcf));
    let base_hue = hue_and_saturation(base).hue;
    let mut best = base_hue;
    let mut best_score = f64::NEG_INFINITY;
    for hue in YOURS_HUES.0..=YOURS_HUES.1 {
        let hue = hue as f64;
        // Clear of both, as near the theme's own hue as that allows; failing
        // that, as far from the nearer of the two as the range goes.
        let score = if apart(hue) >= DISTINCT_HUE {
            1000.0 - hue_distance(hue, base_hue)
        } else {
            apart(hue)
        };
        if score > best_score {
            best_score = score;
            best = hue;
        }
    }
    with_hue(base, best)
}

/// A colour with an alpha, from the opaque colour the web app wrote it as.
fn with_alpha(color: Rgb, alpha: f32) -> Color {
    Color::from_rgba8(color.0, color.1, color.2, alpha)
}

/// Every colour token of the window for this terminal palette and accent.
pub fn derive_colors(appearance: Appearance, accent: Color, terminal: &TerminalColors) -> Colors {
    let accent = Rgb::from(accent);
    let slots = terminal.ansi.map(Rgb::from);
    let [red, green, yellow] = [slots[1], slots[2], slots[3]];
    let bg = Rgb::from(terminal.background);
    let is_dark = appearance.is_dark();
    let lift = if is_dark { Rgb::WHITE } else { Rgb::BLACK };

    // A step from the terminal background toward the foreground side.
    let lifted = |amount: f64| mix(bg, lift, amount);

    let chrome = lifted(0.04);
    let chrome_bar = lifted(0.06);
    let raised = lifted(0.07);
    let pane_head = lifted(0.025);
    let bg_sunken = mix(bg, if is_dark { Rgb::BLACK } else { Rgb::WHITE }, 0.35);

    // Every surface the chrome writes on: the navigator, inspector and status
    // bar, the toolbar, raised groups and cards, pane headers. A colour is held
    // to the least favourable of them: on a dark theme the lightest, on a light
    // one the darkest.
    let surfaces = [chrome, chrome_bar, raised, pane_head];
    // A step further out: the selected row, and a hairline's tint laid over a
    // raised group or the toolbar (a chip, the activity capsule). Whatever can
    // be written there (body text, labels, state words, links, status colours)
    // clears those as well. The two quietest greys answer to the plain surfaces
    // only, so they stay a step below the labels; a rule that puts them on a
    // fill takes --fg-secondary instead.
    let selection = mix(bg, lift, 0.13);
    let line_alpha = if is_dark { 0.07 } else { 0.08 };
    let tinted = mix(raised, lift, line_alpha);
    let anywhere = [chrome, chrome_bar, raised, pane_head, selection, tinted];
    let text =
        |color: Rgb, toward: Option<Rgb>| ensure_contrast(color, &anywhere, TEXT_CONTRAST, toward);
    let fill = |color: Rgb| ensure_contrast(color, &anywhere, FILL_CONTRAST, None);

    // The theme's foreground, unless it is too faint for the chrome
    // (Solarized): body text there leans out to white or black until it reads.
    let fg = text(Rgb::from(terminal.foreground), None);
    // Text a step back from the foreground: `amount` of the way to the
    // background, or nearer the foreground until it reads on `on`. Leaning on
    // the foreground rather than on white keeps the theme's tint in its greys.
    let quieter =
        |amount: f64, on: &[Rgb]| ensure_contrast(mix(fg, bg, amount), on, TEXT_CONTRAST, Some(fg));

    // A light theme's "yellow" is often a dark brown that reads as body text,
    // so light themes start from an amber that still looks like one.
    let amber = if is_dark {
        yellow
    } else {
        Rgb(0xb4, 0x53, 0x09)
    };
    let yours = yours_base(accent, &slots, green);
    let danger = text(red, None);
    // Text on the accent (a primary button, a selected row): the accent
    // darkened almost to black on a dark theme, white on a light one, unless
    // the accent is too light or too dark for that to read; then the other one.
    let dark_on_accent = ensure_contrast(
        mix(accent, Rgb::BLACK, 0.82),
        &[accent],
        TEXT_CONTRAST,
        Some(Rgb::BLACK),
    );
    let (preferred, other) = if is_dark {
        (dark_on_accent, Rgb::WHITE)
    } else {
        (Rgb::WHITE, dark_on_accent)
    };
    let on_accent = if contrast_ratio(preferred, accent) >= TEXT_CONTRAST
        || contrast_ratio(preferred, accent) >= contrast_ratio(other, accent)
    {
        preferred
    } else {
        other
    };

    // The statistics' data colours, all of one hue (the accent's) in steps
    // away from the card they are drawn on: brighter on a dark theme, deeper
    // on a light one. A chart there encodes "how much", and so does
    // lightness, so the page keeps the rest of the chrome's quiet without a
    // palette of its own. The strongest step is the accent held to 3:1 on the
    // card; the faintest still reads at 2:1, the floor for a mark a label or
    // a table also carries.
    let card = if is_dark { raised } else { bg };
    let toward = |top: Rgb, amount: f64| mix(card, top, amount);
    // Where the ramp starts: 40 % of the way to the top, or further when a
    // pale accent needs it to reach 2:1.
    let viz_start_for = |top: Rgb| {
        let mut start = 0.4;
        while start < 0.7 && contrast_ratio(toward(top, start), card) < 2.0 {
            start += 0.01;
        }
        start
    };
    let span_of = |top: Rgb| {
        (perceived_lightness(top) - perceived_lightness(toward(top, viz_start_for(top)))).abs()
    };
    // A ramp too short to tell its four steps apart at a glance (0.065 of
    // perceived lightness each, with room to spare) has its top pushed out,
    // brighter on a dark theme and deeper on a light one, until it is not.
    let mut viz_top = fill(if is_dark {
        mix(accent, Rgb::WHITE, 0.12)
    } else {
        accent
    });
    let mut push = 0;
    while push < 8 && span_of(viz_top) < 0.2 {
        viz_top = mix(viz_top, if is_dark { Rgb::WHITE } else { Rgb::BLACK }, 0.06);
        push += 1;
    }
    // `count` steps from `start` of the way to the top up to the top itself,
    // at even distances of perceived lightness rather than of the mix: a mix
    // toward a vivid colour bunches its last steps together.
    let ramp = |start: f64, count: usize| -> Vec<Rgb> {
        let from = perceived_lightness(toward(viz_top, start));
        let to = perceived_lightness(viz_top);
        (0..count)
            .map(|index| {
                let target = from + (to - from) * index as f64 / (count - 1) as f64;
                let (mut low, mut high) = (start, 1.0);
                for _ in 0..20 {
                    let middle = (low + high) / 2.0;
                    if (perceived_lightness(toward(viz_top, middle)) - target) * (to - from) < 0.0 {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                toward(viz_top, (low + high) / 2.0)
            })
            .collect()
    };
    let series: Vec<Rgb> = ramp(viz_start_for(viz_top), 4)
        .into_iter()
        .map(|color| ensure_contrast(color, &[card], 2.0, Some(viz_top)))
        .collect();
    // The heatmap starts fainter: a quiet hour is meant to be barely there,
    // and its cells carry their values in a tooltip and a table.
    let heat = ramp(0.24, 5);

    let alpha_of_lift = |alpha: f32| with_alpha(lift, alpha);
    let c = |color: Rgb| Color::from(color);

    Colors {
        chrome: c(chrome),
        chrome_bar: c(chrome_bar),
        pane_head: c(pane_head),
        pane_head_focus: c(lifted(0.045)),
        raised: c(raised),
        card: c(card),
        sep: c(mix(bg, Rgb::BLACK, if is_dark { 0.5 } else { 0.14 })),
        line: alpha_of_lift(line_alpha as f32),
        line_strong: alpha_of_lift(if is_dark { 0.12 } else { 0.14 }),
        accent_hi: c(text(
            if is_dark {
                mix(accent, Rgb::WHITE, 0.22)
            } else {
                accent
            },
            None,
        )),
        // An agent's state, always in these four, and always with its shape in
        // the UI. Each comes twice: the word, at 4.5:1 like any text, and the
        // fill of its glyph, a bar or a frame, held only to 3:1 so it keeps
        // the theme's own vivid colour.
        state_working: c(text(green, None)),
        state_working_fill: c(fill(green)),
        state_waiting: c(text(amber, None)),
        state_waiting_fill: c(fill(amber)),
        state_yours: c(text(yours, None)),
        state_yours_fill: c(fill(yours)),
        state_idle: c(quieter(0.55, &anywhere)),
        state_idle_fill: c(fill(mix(fg, bg, 0.55))),
        bg: c(bg),
        bg_sunken: c(bg_sunken),
        surface: c(mix(bg, lift, 0.04)),
        surface_hover: c(mix(bg, lift, 0.08)),
        surface_active: c(selection),
        border: c(mix(bg, lift, 0.11)),
        border_strong: c(mix(bg, lift, 0.2)),
        fg: c(fg),
        // Secondary text in three steps (labels, then metadata, then the
        // quietest hints, paths and counts), each at 4.5:1 on every surface it
        // is drawn on, which on a low-contrast theme draws them closer
        // together.
        fg_secondary: c(quieter(0.24, &anywhere)),
        fg_muted: c(quieter(0.33, &surfaces)),
        fg_subtle: c(quieter(0.5, &surfaces)),
        accent: c(accent),
        accent_soft: with_alpha(accent, 0.14),
        accent_ring: with_alpha(accent, 0.45),
        accent_fg: c(on_accent),
        danger: c(danger),
        danger_soft: with_alpha(danger, 0.14),
        success: c(text(green, None)),
        // Same amber as the waiting state, for the same reason.
        warning: c(text(amber, None)),
        shadow: if is_dark {
            Color::from_rgba8(0, 0, 0, 0.45)
        } else {
            Color::from_rgba8(0, 0, 0, 0.12)
        },
        // What floats over the window gets a dark outer edge under its light
        // hairline on a dark theme, so a panel never melts into what is
        // behind it; on a light theme the shadow does that almost alone.
        float_ring: if is_dark {
            Color::from_rgba8(0, 0, 0, 0.55)
        } else {
            Color::from_rgba8(0, 0, 0, 0.05)
        },
        // A segmented control: a recessed track, and the chosen segment
        // raised out of it, lighter on a dark theme and white on a light one.
        // On a dark theme of little contrast it rises only as far as its
        // label still reads.
        seg_track: c(if is_dark { bg } else { mix(bg, lift, 0.13) }),
        seg_thumb: c(if is_dark {
            ensure_contrast(
                mix(chrome_bar, fg, 0.17),
                &[fg],
                TEXT_CONTRAST,
                Some(chrome_bar),
            )
        } else {
            bg_sunken
        }),
        viz_1: c(series[0]),
        viz_2: c(series[1]),
        viz_3: c(series[2]),
        viz_4: c(series[3]),
        viz_heat_0: c(mix(card, lift, if is_dark { 0.05 } else { 0.06 })),
        viz_heat_1: c(heat[0]),
        viz_heat_2: c(heat[1]),
        viz_heat_3: c(heat[2]),
        viz_heat_4: c(heat[3]),
        viz_heat_5: c(heat[4]),
        viz_mute: c(ensure_contrast(mix(card, fg, 0.22), &[card], 1.6, Some(fg))),
        viz_line: c(ensure_contrast(mix(card, fg, 0.42), &[card], 3.0, Some(fg))),
    }
}
