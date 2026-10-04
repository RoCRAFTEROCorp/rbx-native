//! File › Save to Roblox / Publish to Roblox: uploads the open place as a new
//! version of a Roblox place through `rbx_cloud::Client::publish_place`
//! (`versionType=Saved` keeps it as a saved version, `Published` makes it
//! the live one).
//!
//! The endpoint needs both the universe and the place id. Only the place id
//! is asked for — a bare id or any link `rbx_cloud::place_id_from_link`
//! reads — and the universe is looked up from it, anonymously. The pair is
//! remembered as the file's link (`home::Link`, recorded through
//! `home::remember` like a place opened from Home), so a place downloaded
//! from Roblox publishes back without being asked at all.
//!
//! Every outcome is a row in the Output dock and the Command Bar's label,
//! like a local save; a failure also opens a dialog with Roblox's answer,
//! because a publish the user believes went through is the costly mistake.
//!
//! Roblox's API doesn't update every class (unions, SurfaceAppearance,
//! wraps, Editable*; see [`NOT_UPDATED_BY_PUBLISH`]), so a successful upload
//! of a place holding any adds a warning row naming them.
//!
//! `RBX_STUDIO_PUBLISH_MOCK=ok|<HTTP status>|network` answers both calls
//! with a canned result instead of the network, for scripted captures.

use std::collections::BTreeSet;
use std::path::Path;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_cloud::{ApiKey, Client, CloudError, PublishMode};
use rbx_dom::WeakDom;

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
    /// `resolving` is the token of the lookup in flight for this very dialog:
    /// a lookup whose token isn't here any more (Cancel, Escape, or a fresh
    /// dialog opened since) lands as a no-op — see [`lookup_finished`].
    Link {
        then: Option<PublishMode>,
        error: Option<String>,
        resolving: Option<u64>,
    },
    Failed {
        mode: PublishMode,
        target: Target,
        failure: Failure,
    },
}

/// A failed upload. `unchanged` is whether the place is known to be as it
/// was: true only when the upload never left or Roblox answered with a
/// refusal. A dropped connection, a timeout or an unreadable answer may
/// come after Roblox took the file, so those say so instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Failure {
    pub(super) message: String,
    pub(super) unchanged: bool,
}

impl Failure {
    fn before_sending(message: String) -> Self {
        Failure {
            message,
            unchanged: true,
        }
    }
}

pub(super) struct RobloxPublish {
    input: Entity<InputState>,
    pub(super) dialog: Option<Dialog>,
    /// Text to put in the box and focus it with, on the next frame: both
    /// need a `Window` the menu action that opens the dialog doesn't have.
    prefill: Option<String>,
    /// An upload is in flight; a second one is refused until it answers.
    busy: bool,
    /// The last lookup token handed out; see [`Dialog::Link`].
    lookups: u64,
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
            lookups: 0,
            _subscription: subscription,
        }
    }
}

impl Shell {
    /// The two File menu commands. An unlinked file asks for its place
    /// first and then carries on with `mode`.
    pub(crate) fn upload_to_roblox(&mut self, mode: PublishMode, cx: &mut Context<Self>) {
        match linked_target(&self.path) {
            Some(target) => self.start_upload(target, mode, cx),
            None => self.open_roblox_link(Some(mode), cx),
        }
    }

    /// File › Link to Roblox Place…, and the first step of an unlinked
    /// upload.
    pub(crate) fn open_roblox_link(&mut self, then: Option<PublishMode>, cx: &mut Context<Self>) {
        let current = linked_target(&self.path);
        self.roblox.prefill = Some(current.map_or(String::new(), |t| t.place_id.to_string()));
        self.roblox.dialog = Some(Dialog::Link {
            then,
            error: None,
            resolving: None,
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
            error, resolving, ..
        }) = &mut self.roblox.dialog
        else {
            return;
        };
        if resolving.is_some() {
            return;
        }
        let text = self.roblox.input.read(cx).value().to_string();
        let Some(place_id) = rbx_cloud::place_id_from_link(&text) else {
            *error = Some("That isn\u{2019}t a place ID or a Roblox game link.".to_string());
            cx.notify();
            return;
        };
        self.roblox.lookups += 1;
        let token = self.roblox.lookups;
        *error = None;
        *resolving = Some(token);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { resolve_target(place_id) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.finish_lookup(token, found, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Lands lookup `token`: stores the link and runs the upload it was
    /// opened for, unless the dialog it belonged to is gone.
    fn finish_lookup(
        &mut self,
        token: u64,
        found: Result<(Target, Option<String>), String>,
        cx: &mut Context<Self>,
    ) {
        let Some(((target, name), then)) = lookup_finished(&mut self.roblox.dialog, token, found)
        else {
            return;
        };
        if let Err(message) = link(&self.path, target, name) {
            self.roblox.dialog = Some(Dialog::Link {
                then,
                error: Some(message),
                resolving: None,
            });
            return;
        }
        self.output.push(
            SOURCE,
            Feedback::Output(format!("Linked to place {}", target.place_id)),
        );
        if let Some(mode) = then {
            self.start_upload(target, mode, cx);
        }
    }

    pub(super) fn start_upload(
        &mut self,
        target: Target,
        mode: PublishMode,
        cx: &mut Context<Self>,
    ) {
        if self.roblox.busy {
            self.output.push(
                SOURCE,
                Feedback::Error(format!(
                    "{} place {} didn\u{2019}t start: an upload is already in progress.",
                    verb(mode).0,
                    target.place_id
                )),
            );
            cx.notify();
            return;
        }
        self.roblox.dialog = None;
        // What is on screen, not what the DOM held when typing last paused.
        self.flush_script_edits(cx);
        let bytes = match self.format.encode(&self.dom) {
            Ok(bytes) => bytes,
            Err(message) => {
                let failed = Err(Failure::before_sending(message));
                return self.finish_upload(target, mode, failed, None, cx);
            }
        };
        // Checked on the tree that was encoded: edits made while a slow
        // upload runs aren't in it.
        let warning = not_updated(&self.dom);
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
                shell.finish_upload(target, mode, result, warning, cx)
            });
        })
        .detach();
    }

    fn finish_upload(
        &mut self,
        target: Target,
        mode: PublishMode,
        result: Result<u64, Failure>,
        warning: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.roblox.busy = false;
        let feedback = outcome(target, mode, &result);
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        if let Some(warning) = warning.filter(|_| result.is_ok()) {
            self.output.push(SOURCE, Feedback::Warning(warning));
        }
        if let Err(failure) = result {
            self.roblox.dialog = Some(Dialog::Failed {
                mode,
                target,
                failure,
            });
        }
        cx.notify();
    }
}

/// What lookup `token` coming back does to `dialog`. Only the lookup the
/// open link dialog is still waiting on counts: it closes the dialog and
/// hands back the place to link and the upload to run. A stale one — the
/// dialog was cancelled, or reopened and confirmed again — changes nothing,
/// so a cancelled "Link and publish" never links nor publishes.
fn lookup_finished<T>(
    dialog: &mut Option<Dialog>,
    token: u64,
    found: Result<T, String>,
) -> Option<(T, Option<PublishMode>)> {
    let Some(Dialog::Link {
        then,
        error,
        resolving,
    }) = dialog
    else {
        return None;
    };
    if *resolving != Some(token) {
        return None;
    }
    match found {
        Ok(target) => {
            let then = *then;
            *dialog = None;
            Some((target, then))
        }
        Err(message) => {
            *error = Some(message);
            *resolving = None;
            None
        }
    }
}

/// Classes Roblox's place-publishing API leaves as they were
/// (`creator-docs`, `cloud/guides/usage-place-publishing.md`: EditableImage,
/// EditableMesh, PartOperation, SurfaceAppearance, BaseWrap): edits to them
/// only go live when published from Roblox Studio.
const NOT_UPDATED_BY_PUBLISH: [&str; 9] = [
    "EditableImage",
    "EditableMesh",
    "PartOperation",
    "UnionOperation",
    "NegateOperation",
    "IntersectOperation",
    "SurfaceAppearance",
    "WrapLayer",
    "WrapTarget",
];

/// The warning a successful upload adds when the place holds any of
/// [`NOT_UPDATED_BY_PUBLISH`], naming the ones it holds.
fn not_updated(dom: &WeakDom) -> Option<String> {
    let mut found = BTreeSet::new();
    let mut stack = dom.root_refs().to_vec();
    while let Some(instance) = stack.pop().and_then(|r| dom.get(r)) {
        if let Some(class) = NOT_UPDATED_BY_PUBLISH
            .iter()
            .find(|c| **c == instance.class())
        {
            found.insert(*class);
        }
        stack.extend_from_slice(instance.children());
    }
    (!found.is_empty()).then(|| {
        format!(
            "Note: Roblox doesn\u{2019}t update {} instances through this upload \u{2014} changes to them only go live when published from Roblox Studio.",
            found.into_iter().collect::<Vec<_>>().join(", ")
        )
    })
}

/// The ing-form and past tense each mode's messages use.
fn verb(mode: PublishMode) -> (&'static str, &'static str) {
    match mode {
        PublishMode::Saved => ("Saving to Roblox", "Saved to Roblox"),
        PublishMode::Published => ("Publishing to Roblox", "Published to Roblox"),
    }
}

fn outcome(target: Target, mode: PublishMode, result: &Result<u64, Failure>) -> Feedback {
    match result {
        Ok(version) => Feedback::Output(format!(
            "{} as version {version} of place {}",
            verb(mode).1,
            target.place_id
        )),
        Err(failure) => Feedback::Error(format!(
            "{} failed for place {}: {}",
            verb(mode).0,
            target.place_id,
            failure.message
        )),
    }
}

fn linked_target(path: &Path) -> Option<Target> {
    home::link_of(path).map(|link| Target {
        universe_id: link.universe_id,
        place_id: link.place_id,
    })
}

/// Stores the link, and moves the file to the top of Recent with it.
fn link(path: &Path, target: Target, name: Option<String>) -> Result<(), String> {
    home::remember(RecentPlace {
        path: std::fs::canonicalize(path).unwrap_or(path.to_path_buf()),
        universe_id: Some(target.universe_id),
        place_id: Some(target.place_id),
        name,
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

/// Blocking: the place's universe, anonymously, and its experience's name
/// for Recent's pill. The name needs the key to reach the universe; without
/// it the link is made all the same, unnamed.
fn resolve_target(place_id: u64) -> Result<(Target, Option<String>), String> {
    if mock().is_some() {
        let target = Target {
            universe_id: 1,
            place_id,
        };
        return Ok((target, Some("Mock Experience".to_string())));
    }
    match Client::new(None).universe_of_place(place_id) {
        Ok(Some(universe_id)) => {
            let name = ApiKey::from_env_or_config()
                .and_then(|key| Client::new(Some(key)).universe(universe_id).ok())
                .map(|universe| universe.display_name);
            let target = Target {
                universe_id,
                place_id,
            };
            Ok((target, name))
        }
        Ok(None) => Err(format!("No place with ID {place_id}.")),
        Err(err) => Err(describe(&err)),
    }
}

/// Blocking: one upload, through the stored key.
fn upload(target: Target, bytes: &[u8], mode: PublishMode) -> Result<u64, Failure> {
    if let Some(which) = mock() {
        return upload_with(target, bytes, mode, |_, _, _, _| mocked(&which));
    }
    let Some(key) = ApiKey::from_env_or_config() else {
        return Err(Failure::before_sending(describe(&CloudError::NoApiKey)));
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
) -> Result<u64, Failure> {
    publish(target.universe_id, target.place_id, bytes, mode).map_err(|err| Failure {
        message: describe(&err),
        unchanged: refused(&err),
    })
}

/// Whether `err` means Roblox definitely didn't take the upload: no key to
/// send it with, or a 4xx answer. A 5xx is left out with the network
/// errors — a gateway timing out may sit in front of a publish that landed.
fn refused(err: &CloudError) -> bool {
    match err {
        CloudError::NoApiKey | CloudError::RateLimited { .. } => true,
        CloudError::Http { status, .. } => (400..500).contains(status),
        _ => false,
    }
}

/// Roblox's own reasons for each status the endpoint documents
/// (`creator-docs`, `reference/cloud/universes-api/v1.json`), ahead of the
/// raw error, so a refusal says what to fix.
fn describe(err: &CloudError) -> String {
    let reason = match err {
        CloudError::Http { status: 400, .. } => "Roblox rejected the place file.",
        CloudError::Http { status: 401, .. } => {
            "The API key isn\u{2019}t valid for this place: it needs universe-places:write on this experience, or it may have expired or been revoked \u{2014} Home \u{203a} Manage key."
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
