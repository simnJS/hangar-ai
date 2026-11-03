//! The style functions against the iced widgets they are for.

use iced::widget::button::Status;
use iced::widget::{
    button, checkbox, container, pick_list, progress_bar, scrollable, text, text_input, toggler,
    tooltip,
};
use iced::{Background, Color, Element, Shadow};

use super::toggler as toggler_style;
use super::{brightness, faded};
use super::{button as button_style, checkbox as checkbox_style, container as container_style};
use super::{pick_list as pick_list_style, progress_bar as progress_bar_style, rule};
use super::{scrollable as scrollable_style, text as text_style, text_input as text_input_style};
use crate::{Rgb, TEXT_CONTRAST, contrast_ratio, default_theme, themes};

/// Every style function has to fit the `.style(..)` of the widget it is for;
/// a signature iced would not take fails to compile here.
#[test]
fn the_functions_plug_into_the_widgets_they_are_for() {
    let t = default_theme();
    let _: Element<'_, ()> = button("x").style(button_style::primary(t)).into();
    let _: Element<'_, ()> = button("x").style(button_style::tab(t, true)).into();
    let _: Element<'_, ()> = container(text("x").style(text_style::muted(t)))
        .style(container_style::card(t))
        .into();
    let _: Element<'_, ()> = text_input("", "").style(text_input_style::field(t)).into();
    let _: Element<'_, u8> = pick_list(vec![1_u8, 2], Some(1_u8), |picked| picked)
        .style(pick_list_style::field(t))
        .menu_style(pick_list_style::menu(t))
        .into();
    let _: Element<'_, ()> = scrollable(text("x"))
        .style(scrollable_style::default(t))
        .into();
    let _: Element<'_, ()> = tooltip(text("x"), text("y"), tooltip::Position::Top)
        .style(container_style::tooltip(t))
        .into();
    let _: Element<'_, ()> = checkbox(true).style(checkbox_style::default(t)).into();
    let _: Element<'_, ()> = toggler(true).style(toggler_style::default(t)).into();
    let _: Element<'_, ()> = progress_bar(0.0..=1.0, 0.5)
        .style(progress_bar_style::accent(t))
        .into();
    let _: Element<'_, ()> = iced::widget::rule::horizontal(1).style(rule::sep(t)).into();
}

#[test]
fn a_primary_buttons_label_stays_readable_in_every_state_on_every_theme() {
    for theme in themes() {
        let style = button_style::primary(theme);
        for status in [Status::Active, Status::Hovered, Status::Pressed] {
            let look = style(&iced::Theme::Dark, status);
            let Some(Background::Color(background)) = look.background else {
                panic!("{}: a primary button without a fill", theme.id);
            };
            let ratio = contrast_ratio(Rgb::from(look.text_color), Rgb::from(background));
            // Brightening the accent for the hover costs a little of the 4.5:1
            // the resting state has; it must not cost more than half a point.
            assert!(
                ratio >= TEXT_CONTRAST - 0.5,
                "{}: {status:?} {ratio:.2}",
                theme.id
            );
        }
    }
}

#[test]
fn a_disabled_button_fades_and_loses_its_shadow() {
    let t = default_theme();
    let style = button_style::primary(t);
    let at_rest = style(&iced::Theme::Dark, Status::Active);
    let disabled = style(&iced::Theme::Dark, Status::Disabled);
    assert!(disabled.text_color.a < at_rest.text_color.a);
    assert_eq!(disabled.shadow, Shadow::default());
}

#[test]
fn the_menu_line_under_the_pointer_is_the_accent() {
    let t = default_theme();
    let hovered = button_style::menu_item(t)(&iced::Theme::Dark, Status::Hovered);
    assert_eq!(hovered.background, Some(Background::Color(t.colors.accent)));
    assert_eq!(hovered.text_color, t.colors.accent_fg);
}

#[test]
fn the_chosen_tab_is_raised_and_the_others_are_not() {
    let t = default_theme();
    let chosen = button_style::tab(t, true)(&iced::Theme::Dark, Status::Active);
    let other = button_style::tab(t, false)(&iced::Theme::Dark, Status::Active);
    assert_eq!(
        chosen.background,
        Some(Background::Color(t.colors.seg_thumb))
    );
    assert_eq!(other.background, None);
}

#[test]
fn a_focused_field_takes_the_accent_for_its_edge() {
    let t = default_theme();
    let focused = text_input_style::field(t)(
        &iced::Theme::Dark,
        text_input::Status::Focused { is_hovered: false },
    );
    assert_eq!(focused.border.color, t.colors.accent);
}

#[test]
fn brightness_scales_the_channels_and_stops_at_white() {
    let lighter = brightness(Color::from_rgb(0.5, 0.9, 0.2), 1.08);
    assert!((lighter.r - 0.54).abs() < 1e-6);
    assert!((lighter.g - 0.972).abs() < 1e-6);
    assert_eq!(brightness(Color::WHITE, 2.0), Color::WHITE);
}

#[test]
fn a_disabled_colour_keeps_45_percent_of_its_alpha() {
    assert!((faded(Color::from_rgba(1.0, 1.0, 1.0, 0.5)).a - 0.225).abs() < 1e-6);
}
