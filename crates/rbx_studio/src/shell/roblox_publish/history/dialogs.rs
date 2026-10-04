//! The two dialogs: the restore confirmation, which names the experience,
//! place and version the way the publish confirmation does, and a failed
//! restore with Roblox's answer and Try again.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_cloud::PublishMode;

use crate::launcher::ui::{self, Weight};

use super::model::date_label;
use super::{tokens, Dialog, HistoryWindow};

impl HistoryWindow {
    pub(super) fn dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let target = self.target()?;
        Some(match self.dialog.as_ref()? {
            Dialog::Restore { version } => {
                let version = *version;
                let when = self
                    .history
                    .versions
                    .iter()
                    .find(|v| v.version == version)
                    .map(|v| format!("Version {version} \u{b7} {}", date_label(&v.created_time)))
                    .unwrap_or_else(|| format!("Version {version}"));
                let name = self.link.as_ref().and_then(|l| l.name.clone());
                let body = details(vec![
                    (
                        "Experience",
                        name.unwrap_or_else(|| format!("Universe {}", target.universe_id)),
                    ),
                    ("Place", target.place_id.to_string()),
                    ("Restoring", when),
                ]);
                let cancel = cancel(cx);
                let publish = cx.listener(move |this, _, _, cx| {
                    this.restore(version, PublishMode::Published, cx)
                });
                let save = cx
                    .listener(move |this, _, _, cx| this.restore(version, PublishMode::Saved, cx));
                ui::dialog(
                    560.,
                    ui::dialog_glyph("archive-restore", ui::accent(), ui::wash()),
                    format!("Restore version {version}?"),
                    "Its file is uploaded as a new version of the place, as Roblox\u{2019}s own restore does; nothing is removed from the history. Restore only saves it: players keep joining the version published now. Restore and publish also makes it the version players join.",
                    Some(body),
                    vec![
                        cancel,
                        ui::button("history-restore-publish", "Restore and publish", Weight::Secondary, false)
                            .on_click(publish)
                            .into_any_element(),
                        ui::button("history-restore-confirm", "Restore", Weight::Primary, false)
                            .on_click(save)
                            .into_any_element(),
                    ],
                )
                .into_any_element()
            }
            Dialog::Failed {
                version,
                mode,
                failure,
            } => {
                let (version, mode) = (*version, *mode);
                let body = ui::mono(11.5, 17.)
                    .mt(px(14.))
                    .mx(px(20.))
                    .ml(px(78.))
                    .p(px(10.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(ui::red_line())
                    .bg(ui::red_soft())
                    .child(failure.message.clone())
                    .into_any_element();
                ui::dialog(
                    560.,
                    ui::dialog_glyph("circle-alert", ui::red(), ui::red_soft()),
                    format!("Restoring version {version} failed"),
                    if failure.unchanged {
                        format!("Place {} was not changed on Roblox.", target.place_id)
                    } else {
                        format!(
                            "The upload to place {} may or may not have been applied \u{2014} refresh the list before trying again.",
                            target.place_id
                        )
                    },
                    Some(body),
                    vec![
                        cancel(cx),
                        ui::button("history-restore-retry", "Try again", Weight::Primary, false)
                            .on_click(cx.listener(move |this, _, _, cx| this.restore(version, mode, cx)))
                            .into_any_element(),
                    ],
                )
                .into_any_element()
            }
        })
    }
}

/// The publish confirmation's label/value box.
fn details(rows: Vec<(&'static str, String)>) -> AnyElement {
    v_flex()
        .mt(px(14.))
        .mx(px(20.))
        .ml(px(78.))
        .p(px(10.))
        .gap(px(4.))
        .rounded(px(6.))
        .border_1()
        .border_color(tokens::border())
        .bg(ui::panel2())
        .children(rows.into_iter().map(|(label, value)| {
            h_flex()
                .gap(px(12.))
                .child(
                    ui::text(12., 18.)
                        .w(px(84.))
                        .flex_none()
                        .text_color(tokens::text2())
                        .child(label),
                )
                .child(ui::text(12., 18.).min_w_0().truncate().child(value))
        }))
        .into_any_element()
}

fn cancel(cx: &mut Context<HistoryWindow>) -> AnyElement {
    ui::button("history-cancel", "Cancel", Weight::Secondary, false)
        .on_click(cx.listener(|this, _, _, cx| {
            this.dialog = None;
            cx.notify();
        }))
        .into_any_element()
}
