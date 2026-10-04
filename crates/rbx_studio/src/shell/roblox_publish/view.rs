//! The link and failure dialogs, in the launcher's own dialog chrome.

use gpui_kit::component::input::Input;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_cloud::PublishMode;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::{verb, Dialog, Shell};

impl Shell {
    pub(in crate::shell) fn roblox_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(match self.roblox.dialog.as_ref()? {
            Dialog::Link {
                then,
                error,
                resolving,
            } => {
                let border = if error.is_some() {
                    ui::red_line()
                } else {
                    tokens::border2()
                };
                let body = v_flex()
                    .gap(px(6.))
                    .pt(px(16.))
                    .px(px(20.))
                    .pl(px(78.))
                    .child(
                        ui::field_frame(None, ui::panel2(), border, "link").child(
                            Input::new(&self.roblox.input)
                                .appearance(false)
                                .disabled(*resolving)
                                .flex_1()
                                .h_full()
                                .px(px(0.))
                                .font_family(tokens::FONT_FAMILY_MONO)
                                .text_size(px(11.5))
                                .text_color(tokens::text()),
                        ),
                    )
                    .when_some(error.clone(), |this, error| {
                        this.child(ui::text(12., 17.).text_color(ui::red()).child(error))
                    })
                    .into_any_element();
                let go = match then {
                    Some(PublishMode::Saved) => "Link and save",
                    Some(PublishMode::Published) => "Link and publish",
                    None => "Link",
                };
                let confirm = if *resolving {
                    ui::disabled_button("roblox-link-go", "Looking up place\u{2026}")
                        .into_any_element()
                } else {
                    ui::button("roblox-link-go", go, Weight::Primary, false)
                        .on_click(cx.listener(|shell, _, _, cx| shell.confirm_roblox_link(cx)))
                        .into_any_element()
                };
                ui::dialog(
                    520.,
                    ui::dialog_glyph("link", ui::accent(), ui::wash()),
                    "Link this file to a Roblox place",
                    "Save to Roblox and Publish to Roblox upload it as a new version of this place. Paste its ID, its game page link, or its Creator Dashboard link.",
                    Some(body),
                    vec![cancel(cx), confirm],
                )
                .into_any_element()
            }
            Dialog::Failed {
                mode,
                target,
                message,
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
                    .child(message.clone())
                    .into_any_element();
                ui::dialog(
                    520.,
                    ui::dialog_glyph("circle-alert", ui::red(), ui::red_soft()),
                    format!("{} failed", verb(mode).0),
                    format!(
                        "Place {} was not changed on Roblox. Your local file is untouched.",
                        target.place_id
                    ),
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
