//! The Dragger & snapping page: Studio's dragger guides, the two drag
//! behaviours, and the Snap popover's increments.

use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::settings::DraggerSettings;
use crate::tokens;
use crate::transform::{self, SnapKind, Transform};

use super::super::Shell;
use super::kit::{toggle, Row, Section};
use super::SettingsWindow;

/// The increment fields: committed on Enter or blur, and rewritten from the
/// setting whenever it changed elsewhere and the field isn't focused.
pub(super) struct Increments {
    translate: Entity<InputState>,
    rotate: Entity<InputState>,
}

/// One dragger switch: which field of [`DraggerSettings`] it flips.
type Switch = fn(&mut DraggerSettings) -> &mut bool;

impl Increments {
    pub(super) fn new(
        shell: &Entity<Shell>,
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) -> (Self, Vec<Subscription>) {
        let mut subscriptions = Vec::new();
        let mut field = |kind: SnapKind, cx: &mut Context<SettingsWindow>| {
            let value = shell.read(cx).snap_increment(kind);
            let input = cx.new(|cx| InputState::new(window, cx).default_value(format!("{value}")));
            // On Enter or blur; a zero or text that isn't a number leaves the
            // increment alone.
            subscriptions.push(commit_on_enter(
                &input,
                window,
                cx,
                move |this, text, cx| {
                    if let Some(increment) = transform::parse_increment(text).filter(|v| *v > 0.) {
                        this.shell.update(cx, |shell, cx| {
                            shell.set_snap_increment(kind, increment, cx)
                        });
                    }
                    this.shell.read(cx).snap_increment(kind)
                },
            ));
            input
        };
        let increments = Increments {
            translate: field(SnapKind::Translate, cx),
            rotate: field(SnapKind::Rotate, cx),
        };
        (increments, subscriptions)
    }

    fn sync(&self, [translate, rotate]: [f32; 2], window: &mut Window, cx: &mut App) {
        for (input, increment) in [(&self.translate, translate), (&self.rotate, rotate)] {
            let state = input.read(cx);
            if state.focus_handle(cx).is_focused(window)
                || transform::parse_increment(&state.value()) == Some(increment)
            {
                continue;
            }
            input.update(cx, |state, cx| {
                state.set_value(format!("{increment}"), window, cx)
            });
        }
    }
}

impl SettingsWindow {
    pub(super) fn dragger_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Section> {
        let (dragger, translate, rotate) = {
            let shell = self.shell.read(cx);
            (
                shell.dragger(),
                shell.snap_increment(SnapKind::Translate),
                shell.snap_increment(SnapKind::Rotate),
            )
        };
        self.increments.sync([translate, rotate], window, cx);
        let focused = [&self.increments.translate, &self.increments.rotate]
            .map(|input| input.read(cx).focus_handle(cx).is_focused(window));
        let defaults = DraggerSettings::default();
        let switch = |id: &'static str, label, description, field: Switch| {
            let on = *field(&mut { dragger });
            let default = *field(&mut { defaults });
            let flip = move |shell: &mut Shell, on: bool, cx: &mut Context<Shell>| {
                let mut settings = shell.dragger();
                *field(&mut settings) = on;
                shell.set_dragger(settings, cx);
            };
            Row::new(
                label,
                toggle(id, on, self.set(move |shell, cx| flip(shell, !on, cx))),
            )
            .describe(description)
            .changed(on != default, move |shell, cx| flip(shell, default, cx))
        };
        let dragging = Section::new(
            "While dragging",
            vec![
                switch(
                    "hover-ruler",
                    "Hover ruler",
                    "Distance to the part under the cursor.",
                    |s| &mut s.show_hover_ruler,
                ),
                switch(
                    "target-snap",
                    "Target snap",
                    "Highlight the face or edge a drag will snap to.",
                    |s| &mut s.show_target_snap,
                ),
                switch(
                    "measurement",
                    "Measurement",
                    "Live size and offset next to the cursor.",
                    |s| &mut s.show_measurement,
                ),
                switch(
                    "dragged-point",
                    "Dragged point",
                    "Mark the point of the part you grabbed.",
                    |s| &mut s.show_dragged_point,
                ),
            ],
        );
        let defaults = Transform::default();
        let snapping = Section::new(
            "Snapping",
            vec![
                switch(
                    "snap-to-parts",
                    "Snap to parts",
                    "Stick to other parts\u{2019} faces and edges.",
                    |s| &mut s.snap_to_parts,
                ),
                switch(
                    "align-dragged",
                    "Align dragged objects",
                    "Rotate the dragged part to match the surface under it.",
                    |s| &mut s.align_dragged_objects,
                ),
                Row::new(
                    "Move increment",
                    number(&self.increments.translate, "studs", focused[0]),
                )
                .describe("Also in the ribbon\u{2019}s Snap popover.")
                .changed(
                    translate != defaults.translate.increment,
                    move |shell, cx| {
                        shell.set_snap_increment(
                            SnapKind::Translate,
                            defaults.translate.increment,
                            cx,
                        )
                    },
                ),
                Row::new(
                    "Rotate increment",
                    number(&self.increments.rotate, "\u{b0}", focused[1]),
                )
                .describe("Snap angle for the Rotate tool.")
                .changed(rotate != defaults.rotate.increment, move |shell, cx| {
                    shell.set_snap_increment(SnapKind::Rotate, defaults.rotate.increment, cx)
                }),
            ],
        );
        vec![dragging, snapping]
    }
}

/// An 80×30 mono field, right-aligned, and its unit after it when it has
/// one.
pub(super) fn number(
    input: &Entity<InputState>,
    unit: &'static str,
    focused: bool,
) -> impl IntoElement {
    h_flex()
        .gap(px(8.))
        .items_center()
        .child(
            h_flex()
                .w(px(80.))
                .h(px(30.))
                .px(px(10.))
                .items_center()
                .border_1()
                .border_color(if focused {
                    tokens::accent_line()
                } else {
                    tokens::border2()
                })
                .rounded(px(6.))
                .bg(tokens::dock())
                .font_family(tokens::FONT_FAMILY_MONO)
                .child(
                    Input::new(input)
                        .appearance(false)
                        .w_full()
                        .px_0()
                        .text_right()
                        .text_size(px(11.5))
                        .line_height(px(16.))
                        .text_color(tokens::text()),
                ),
        )
        .when(!unit.is_empty(), |this| {
            this.child(
                div()
                    .text_size(px(12.))
                    .text_color(tokens::text2())
                    .child(unit),
            )
        })
}

/// Commits a [`number`] field on Enter or blur, not per keystroke, so the
/// `8` on the way to `80` never lands, and only once it was typed in, so
/// leaving an untouched field doesn't put back a value reset meanwhile.
/// `commit` applies what it accepts of the text and returns the value then
/// in effect, which the field is rewritten to: a clamped or refused entry
/// shows what was kept.
pub(super) fn commit_on_enter(
    input: &Entity<InputState>,
    window: &Window,
    cx: &mut Context<SettingsWindow>,
    commit: impl Fn(&mut SettingsWindow, &str, &mut Context<SettingsWindow>) -> f32 + 'static,
) -> Subscription {
    let mut edited = false;
    cx.subscribe_in(
        input,
        window,
        move |this, input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => edited = true,
            InputEvent::PressEnter { .. } | InputEvent::Blur if edited => {
                edited = false;
                let text = input.read(cx).value().to_string();
                let kept = format!("{}", commit(this, &text, cx));
                if text.trim() != kept {
                    input.update(cx, |state, cx| state.set_value(kept, window, cx));
                }
            }
            _ => {}
        },
    )
}

/// A committed field's number: the text read as one, clamped into `range`
/// when `clamp`, refused outside it otherwise — each setting keeping the
/// rule it loads with.
pub(super) fn committed(text: &str, (low, high): (f32, f32), clamp: bool) -> Option<f32> {
    let value = text.trim().parse::<f32>().ok().filter(|v| v.is_finite())?;
    if clamp {
        Some(value.clamp(low, high))
    } else {
        (low..=high).contains(&value).then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::committed;

    #[test]
    fn committed_clamps_or_refuses_the_whole_entry() {
        let font = crate::settings::SCRIPT_FONT_SIZE_RANGE;
        assert_eq!(committed("80", font, true), Some(32.));
        assert_eq!(committed("2", font, true), Some(8.));
        assert_eq!(committed(" 16 ", font, true), Some(16.));
        let sun = crate::settings::calibration_range(1.);
        assert_eq!(committed("1000", sun, false), None);
        assert_eq!(committed("5", sun, false), Some(5.));
        assert_eq!(committed("", font, true), None);
        assert_eq!(committed("inf", font, true), None);
        assert_eq!(committed("NaN", sun, false), None);
    }
}
