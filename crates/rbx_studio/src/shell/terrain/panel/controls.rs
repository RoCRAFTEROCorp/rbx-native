//! The panel's building blocks: sections, labelled rows, switches, chip
//! groups, number fields, sliders and buttons, in the dock's own skin
//! (`UX_GUIDELINES.md`'s tokens).

use gpui_kit::component::input::InputState;
use gpui_kit::component::slider::SliderState;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::super::super::rows::{rail_slider, text_field, toggle_pill};
use super::super::super::Shell;

/// A section: its small uppercase header, then its rows.
pub(super) fn section(title: &'static str, rows: Vec<AnyElement>) -> Div {
    v_flex()
        .gap(px(8.))
        .child(
            div()
                .text_size(tokens::text_xxs())
                .line_height(tokens::line_xxs())
                .font_weight(tokens::WEIGHT_BOLD)
                .text_color(tokens::text3())
                .child(title),
        )
        .children(rows)
}

fn label(text: impl Into<SharedString>) -> Div {
    div()
        .min_w_0()
        .truncate()
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .text_color(tokens::text2())
        .child(text.into())
}

/// A switch row: the whole row is the target.
pub(super) fn switch_row(
    id: impl Into<SharedString>,
    text: &'static str,
    on: bool,
    cx: &mut Context<Shell>,
    set: impl Fn(&mut Shell, bool, &mut Context<Shell>) + 'static,
) -> AnyElement {
    h_flex()
        .id(ElementId::Name(id.into()))
        .h(tokens::row_height())
        .items_center()
        .justify_between()
        .gap(px(12.))
        .px(px(6.))
        .mx(px(-6.))
        .rounded(tokens::radius_badge())
        .cursor_pointer()
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover_subtle()))
        .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
        .on_click(cx.listener(move |shell, _, _, cx| set(shell, !on, cx)))
        .child(label(text))
        .child(toggle_pill(Some(on)))
        .into_any_element()
}

/// A row of mutually exclusive chips, one per choice.
pub(super) fn chips<T: Copy + PartialEq + 'static>(
    id: &'static str,
    choices: &[(T, &'static str)],
    current: T,
    cx: &mut Context<Shell>,
    pick: fn(&mut Shell, T, &mut Context<Shell>),
) -> AnyElement {
    h_flex()
        .w_full()
        .p(px(2.))
        .gap(px(2.))
        .rounded(tokens::radius_segment())
        .bg(tokens::field_select())
        .children(choices.iter().map(|&(value, text)| {
            let on = value == current;
            div()
                .id(SharedString::from(format!("{id}-{text}")))
                .flex_1()
                .h(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens::radius_tiny())
                .cursor_pointer()
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(if on { tokens::text() } else { tokens::text2() })
                .when(on, |this| this.bg(tokens::accent_soft()))
                .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
                .on_click(cx.listener(move |shell, _, _, cx| pick(shell, value, cx)))
                .child(text)
        }))
        .into_any_element()
}

/// A label over one to three number fields (X, Y, Z).
pub(super) fn number_row(
    text: &'static str,
    inputs: Vec<Entity<InputState>>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    v_flex()
        .gap(px(4.))
        .child(label(text))
        .child(
            h_flex().gap(px(6.)).children(
                inputs
                    .iter()
                    .map(|input| div().flex_1().min_w_0().child(text_field(input, 0, cx))),
            ),
        )
        .into_any_element()
}

/// A label, its value, and the slider beneath.
pub(super) fn rail_row(
    text: &'static str,
    state: &Entity<SliderState>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let value = state.read(cx).value().end();
    let shown = if value.fract().abs() < 1e-4 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    v_flex()
        .gap(px(2.))
        .child(
            h_flex().justify_between().child(label(text)).child(
                div()
                    .text_color(tokens::text3())
                    .text_size(tokens::text_sm())
                    .child(shown),
            ),
        )
        .child(rail_slider(state, cx))
        .into_any_element()
}

/// A button; `primary` wears the accent.
pub(super) fn action_button(
    id: &'static str,
    text: &'static str,
    primary: bool,
    cx: &mut Context<Shell>,
    run: fn(&mut Shell, &mut Context<Shell>),
) -> AnyElement {
    div()
        .id(id)
        .h(px(30.))
        .px(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::radius())
        .cursor_pointer()
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .font_weight(tokens::WEIGHT_BOLD)
        .map(|this| {
            if primary {
                this.bg(tokens::check_on()).text_color(tokens::knob())
            } else {
                this.bg(tokens::field_select()).text_color(tokens::text2())
            }
        })
        .hover(|this| {
            tokens::hover_fx(this).bg(if primary {
                tokens::accent_hover()
            } else {
                tokens::hover()
            })
        })
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .on_click(cx.listener(move |shell, _, _, cx| run(shell, cx)))
        .child(text)
        .into_any_element()
}

/// A file slot: its label, the chosen file's name, and a button to choose.
pub(super) fn file_row(
    text: &'static str,
    file: String,
    id: &'static str,
    cx: &mut Context<Shell>,
    choose: fn(&mut Shell, &mut Context<Shell>),
) -> AnyElement {
    h_flex()
        .gap(px(8.))
        .items_center()
        .justify_between()
        .child(
            v_flex().min_w_0().child(label(text)).child(
                div()
                    .truncate()
                    .text_color(tokens::text3())
                    .text_size(tokens::text_sm())
                    .child(file),
            ),
        )
        .child(action_button(id, "Choose…", false, cx, choose))
        .into_any_element()
}
