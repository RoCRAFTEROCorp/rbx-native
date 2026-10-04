//! The upload confirmation and the failure dialog, in the launcher's own
//! dialog chrome.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_cloud::PublishMode;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::{verb, Dialog, Shell};

impl Shell {
    pub(in crate::shell) fn roblox_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(match self.roblox.dialog.as_ref()? {
            Dialog::Confirm { mode, target, name } => {
                let (title, go, what) = match mode {
                    PublishMode::Published => (
                        "Publish to Roblox?",
                        "Publish",
                        "This file is uploaded as a new version of the place and published: it becomes the version players join.",
                    ),
                    PublishMode::Saved => (
                        "Save to Roblox?",
                        "Save",
                        "This file is uploaded as a new saved version of the place, without publishing it: players keep joining the version published now.",
                    ),
                };
                let row = |label: &'static str, value: String| {
                    h_flex()
                        .gap(px(12.))
                        .child(
                            ui::text(12., 18.)
                                .w(px(84.))
                                .flex_none()
                                .text_color(tokens::text2())
                                .child(label),
                        )
                        .child(
                            ui::text(12., 18.)
                                .min_w_0()
                                .truncate()
                                .text_color(tokens::text())
                                .child(value),
                        )
                };
                let body = v_flex()
                    .mt(px(14.))
                    .mx(px(20.))
                    .ml(px(78.))
                    .p(px(10.))
                    .gap(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(tokens::border())
                    .bg(ui::panel2())
                    .child(row(
                        "Experience",
                        name.clone()
                            .unwrap_or_else(|| format!("Universe {}", target.universe_id)),
                    ))
                    .child(row("Place", target.place_id.to_string()))
                    .into_any_element();
                // Covers the window like the veil inside it, so the dialog
                // is placed as it would be without the focus wrapper.
                div()
                    .absolute()
                    .inset_0()
                    .track_focus(&self.roblox.focus)
                    .child(ui::dialog(
                        520.,
                        ui::dialog_glyph("cloud-upload", ui::accent(), ui::wash()),
                        title,
                        format!("{what} Your local file isn\u{2019}t changed."),
                        Some(body),
                        vec![
                            cancel(cx),
                            ui::button("roblox-confirm", go, Weight::Primary, false)
                                .on_click(
                                    cx.listener(|shell, _, _, cx| shell.confirm_roblox_upload(cx)),
                                )
                                .into_any_element(),
                        ],
                    ))
                    .into_any_element()
            }
            Dialog::Failed {
                mode,
                target,
                failure,
            } => {
                let (mode, target) = (*mode, *target);
                let body = ui::mono(11.5, 17.)
                    .mt(px(14.))
                    .mx(px(20.))
                    .ml(px(78.))
                    .p(px(10.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(ui::red_line())
                    .bg(ui::red_soft())
                    .text_color(tokens::text())
                    .child(failure.message.clone())
                    .into_any_element();
                ui::dialog(
                    520.,
                    ui::dialog_glyph("circle-alert", ui::red(), ui::red_soft()),
                    format!("{} failed", verb(mode).0),
                    if failure.unchanged {
                        format!(
                            "Place {} was not changed on Roblox. Your local file is untouched.",
                            target.place_id
                        )
                    } else {
                        format!(
                            "The upload to place {} may or may not have been applied \u{2014} check its versions on the Creator Dashboard before trying again. Your local file is untouched.",
                            target.place_id
                        )
                    },
                    Some(body),
                    vec![
                        ui::button(
                            "roblox-relink",
                            "Change place\u{2026}",
                            Weight::Ghost,
                            false,
                        )
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.open_roblox_link(Some(mode), cx)
                        }))
                        .into_any_element(),
                        cancel(cx),
                        ui::button("roblox-retry", "Try again", Weight::Primary, false)
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                shell.start_upload(target, mode, cx)
                            }))
                            .into_any_element(),
                    ],
                )
                .into_any_element()
            }
        })
    }
}

fn cancel(cx: &mut Context<Shell>) -> AnyElement {
    ui::button("roblox-cancel", "Cancel", Weight::Secondary, false)
        .on_click(cx.listener(|shell, _, _, cx| {
            shell.close_roblox_dialog();
            cx.notify();
        }))
        .into_any_element()
}
