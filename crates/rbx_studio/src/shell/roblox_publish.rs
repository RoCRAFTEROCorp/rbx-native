//! File › Save to Roblox / Publish to Roblox: uploads the open place as a new
//! version of a Roblox place through `rbx_cloud::Client::publish_place`
//! (`versionType=Saved` keeps it as a saved version, `Published` makes it
//! the live one).
//!
//! The endpoint needs both the universe and the place id. Only the place id
//! is asked for — a bare id or any link `rbx_cloud::place_id_from_link`
//! reads — and the universe is looked up from it, anonymously. The pair is
//! remembered as the file's entry in Recent (`home::RecentPlace`), which is
//! where a place opened from Home already carries its ids, so a place
//! downloaded from Roblox publishes back without being asked at all.
//!
//! Every outcome is a row in the Output dock and the Command Bar's label,
//! like a local save; a failure also opens a dialog with Roblox's answer,
//! because a publish the user believes went through is the costly mistake.
//!
//! `RBX_STUDIO_PUBLISH_MOCK=ok|<HTTP status>|network` answers both calls
//! with a canned result instead of the network, for scripted captures.

use std::path::Path;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_cloud::{ApiKey, Client, CloudError, PublishMode};

use crate::command_bar::Feedback;
use crate::home::{self, RecentPlace};

use super::Shell;

mod view;

const MOCK_VARIABLE: &str = "RBX_STUDIO_PUBLISH_MOCK";

/// The Output dock's `source` for every row this module pushes.
const SOURCE: &str = "Roblox";

/// The Roblox place a file publishes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Target {
    universe_id: u64,
    place_id: u64,
}

pub(super) enum Dialog {
    /// Asking for the place. `then` is the upload the link was opened for,
    /// run as soon as the link is stored; `None` when the user only relinks.
    Link {
        then: Option<PublishMode>,
        error: Option<String>,
        resolving: bool,
    },
    Failed {
        mode: PublishMode,
        target: Target,
        message: String,
    },
}

pub(super) struct RobloxPublish {
    input: Entity<InputState>,
    pub(super) dialog: Option<Dialog>,
    /// Text to put in the box and focus it with, on the next frame: both
    /// need a `Window` the menu action that opens the dialog doesn't have.
    prefill: Option<String>,
    /// An upload is in flight; a second one is refused until it answers.
    busy: bool,
    _subscription: Subscription,
}

impl RobloxPublish {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Place ID or link"));
        let subscription = cx.subscribe(&input, |shell, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                shell.confirm_roblox_link(cx);
            }
        });
        RobloxPublish {
            input,
            dialog: None,
            prefill: None,
            busy: false,
            _subscription: subscription,
        }
    }
}

impl Shell {
    /// The two File menu commands. An unlinked file asks for its place
    /// first and then carries on with `mode`.
    pub(crate) fn upload_to_roblox(&mut self, mode: PublishMode, cx: &mut Context<Self>) {
        match linked_target(&home::recent(), &self.path) {
            Some(target) => self.start_upload(target, mode, cx),
            None => self.open_roblox_link(Some(mode), cx),
        }
    }

    /// File › Link to Roblox Place…, and the first step of an unlinked
    /// upload.
    pub(crate) fn open_roblox_link(&mut self, then: Option<PublishMode>, cx: &mut Context<Self>) {
        let current = linked_target(&home::recent(), &self.path);
        self.roblox.prefill = Some(current.map_or(String::new(), |t| t.place_id.to_string()));
        self.roblox.dialog = Some(Dialog::Link {
            then,
            error: None,
            resolving: false,
        });
        cx.notify();
    }

    /// Escape's half of the dialog; returns whether one was open.
    pub(super) fn close_roblox_dialog(&mut self) -> bool {
        self.roblox.dialog.take().is_some()
    }

    /// Runs from render, where a `Window` is at hand; see [`RobloxPublish::prefill`].
    pub(super) fn focus_roblox_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.roblox.prefill.take() else {
            return;
        };
        self.roblox.input.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.focus(window, cx);
            state.select_all(window, cx);
        });
    }

    fn confirm_roblox_link(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Link {
            then,
            error,
            resolving,
        }) = &mut self.roblox.dialog
        else {
            return;
        };
        if *resolving {
            return;
        }
        let then = *then;
        let text = self.roblox.input.read(cx).value().to_string();
        let Some(place_id) = rbx_cloud::place_id_from_link(&text) else {
            *error = Some("That isn\u{2019}t a place ID or a Roblox game link.".to_string());
            cx.notify();
            return;
        };
        *error = None;
        *resolving = true;
        cx.notify();
        let path = self.path.clone();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { resolve_target(place_id) })
                .await
                .and_then(|target| link(&path, target).map(|()| target));
            let _ = this.update(cx, |shell, cx| match found {
                Ok(target) => {
                    shell.roblox.dialog = None;
                    shell.output.push(
                        SOURCE,
                        Feedback::Output(format!("Linked to place {place_id}")),
                    );
                    if let Some(mode) = then {
                        shell.start_upload(target, mode, cx);
                    }
                    cx.notify();
                }
                Err(message) => {
                    if let Some(Dialog::Link {
                        error, resolving, ..
                    }) = &mut shell.roblox.dialog
                    {
                        *error = Some(message);
                        *resolving = false;
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn start_upload(
        &mut self,
        target: Target,
        mode: PublishMode,
        cx: &mut Context<Self>,
    ) {
        if self.roblox.busy {
            return;
        }
        self.roblox.dialog = None;
        // What is on screen, not what the DOM held when typing last paused.
        self.flush_script_edits(cx);
        let bytes = match self.format.encode(&self.dom) {
            Ok(bytes) => bytes,
            Err(message) => return self.finish_upload(target, mode, Err(message), cx),
        };
        self.roblox.busy = true;
        self.command_bar.set_feedback(Feedback::Output(format!(
            "{} place {}\u{2026}",
            verb(mode).0,
            target.place_id
        )));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { upload(target, &bytes, mode) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.finish_upload(target, mode, result, cx)
            });
        })
        .detach();
    }

    fn finish_upload(
        &mut self,
        target: Target,
        mode: PublishMode,
        result: Result<u64, String>,
        cx: &mut Context<Self>,
    ) {
        self.roblox.busy = false;
        let feedback = outcome(target, mode, &result);
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        if let Err(message) = result {
            self.roblox.dialog = Some(Dialog::Failed {
                mode,
                target,
                message,
            });
        }
        cx.notify();
    }
}

/// The ing-form and past tense each mode's messages use.
fn verb(mode: PublishMode) -> (&'static str, &'static str) {
    match mode {
        PublishMode::Saved => ("Saving to Roblox", "Saved to Roblox"),
        PublishMode::Published => ("Publishing to Roblox", "Published to Roblox"),
    }
}

fn outcome(target: Target, mode: PublishMode, result: &Result<u64, String>) -> Feedback {
    match result {
        Ok(version) => Feedback::Output(format!(
            "{} as version {version} of place {}",
            verb(mode).1,
            target.place_id
        )),
        Err(message) => Feedback::Error(format!(
            "{} failed for place {}: {message}",
            verb(mode).0,
            target.place_id
        )),
    }
}

/// The file's entry in Recent, when it carries both ids. Paths are compared
/// canonicalized: the editor records its file that way, Home records the
/// download path as it built it.
fn linked_target(recent: &[RecentPlace], path: &Path) -> Option<Target> {
    let path = std::fs::canonicalize(path).unwrap_or(path.to_path_buf());
    recent
        .iter()
        .find(|place| std::fs::canonicalize(&place.path).unwrap_or(place.path.clone()) == path)
        .and_then(|place| {
            Some(Target {
                universe_id: place.universe_id?,
                place_id: place.place_id?,
            })
        })
}

/// ponytail: the link lives in Recent, which keeps 20 files; a file that
/// falls off it is asked for its place again. Its own map if that bites.
fn link(path: &Path, target: Target) -> Result<(), String> {
    home::remember(RecentPlace {
        path: std::fs::canonicalize(path).unwrap_or(path.to_path_buf()),
        universe_id: Some(target.universe_id),
        place_id: Some(target.place_id),
        name: None,
        opened: None,
    })
    .map_err(|err| format!("the link couldn\u{2019}t be saved: {err}"))
}

fn mock() -> Option<String> {
    std::env::var(MOCK_VARIABLE).ok()
}

/// What a mocked call answers instead of Roblox.
fn mocked(which: &str) -> Result<u64, CloudError> {
    match which {
        "ok" => Ok(7),
        "network" => Err(CloudError::Transport("connection refused".to_string())),
        status => Err(CloudError::Http {
            status: status.parse().unwrap_or(500),
            url: "https://apis.roblox.com/universes/v1/…/versions?<redacted>".to_string(),
        }),
    }
}

/// Blocking: the place's universe, anonymously.
fn resolve_target(place_id: u64) -> Result<Target, String> {
    if mock().is_some() {
        return Ok(Target {
            universe_id: 1,
            place_id,
        });
    }
    match Client::new(None).universe_of_place(place_id) {
        Ok(Some(universe_id)) => Ok(Target {
            universe_id,
            place_id,
        }),
        Ok(None) => Err(format!("No place with ID {place_id}.")),
        Err(err) => Err(describe(&err)),
    }
}

/// Blocking: one upload, through the stored key.
fn upload(target: Target, bytes: &[u8], mode: PublishMode) -> Result<u64, String> {
    if let Some(which) = mock() {
        return upload_with(target, bytes, mode, |_, _, _, _| mocked(&which));
    }
    let Some(key) = ApiKey::from_env_or_config() else {
        return Err(describe(&CloudError::NoApiKey));
    };
    let client = Client::new(Some(key));
    upload_with(target, bytes, mode, |universe, place, bytes, mode| {
        client.publish_place(universe, place, bytes, mode)
    })
}

/// The seam the tests drive: `publish` stands in for `Client::publish_place`.
fn upload_with(
    target: Target,
    bytes: &[u8],
    mode: PublishMode,
    publish: impl FnOnce(u64, u64, &[u8], PublishMode) -> Result<u64, CloudError>,
) -> Result<u64, String> {
    publish(target.universe_id, target.place_id, bytes, mode).map_err(|err| describe(&err))
}

/// Roblox's own reasons for each status the endpoint documents
/// (`creator-docs`, `reference/cloud/universes-api/v1.json`), ahead of the
/// raw error, so a refusal says what to fix.
fn describe(err: &CloudError) -> String {
    let reason = match err {
        CloudError::Http { status: 400, .. } => "Roblox rejected the place file.",
        CloudError::Http { status: 401, .. } => {
            "The API key isn\u{2019}t valid for this place: it needs universe-places:write on this experience."
        }
        CloudError::Http { status: 403, .. } => "Publishing isn\u{2019}t allowed on this place.",
        CloudError::Http { status: 404, .. } => "The place or its experience doesn\u{2019}t exist.",
        CloudError::Http { status: 409, .. } => "The place isn\u{2019}t part of that experience.",
        CloudError::NoApiKey => {
            return "No Open Cloud API key is set up. Add one from Home \u{203a} Manage key.".to_string()
        }
        _ => return err.to_string(),
    };
    format!("{reason} ({err})")
}

#[cfg(test)]
#[path = "roblox_publish/tests.rs"]
mod tests;
