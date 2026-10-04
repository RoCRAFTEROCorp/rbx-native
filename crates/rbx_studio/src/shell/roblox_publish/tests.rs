use rbx_cloud::{CloudError, PublishMode};

use super::{
    describe, lookup_finished, mocked, not_updated, outcome, refused, upload_with, Dialog, Failure,
    Target,
};
use crate::command_bar::Feedback;

const TARGET: Target = Target {
    universe_id: 6053515322,
    place_id: 17675488706,
};

#[test]
fn save_and_publish_reach_the_client_with_their_own_mode_and_the_linked_ids() {
    for mode in [PublishMode::Saved, PublishMode::Published] {
        let mut seen = None;
        let result = upload_with(TARGET, b"<roblox!", mode, |universe, place, bytes, sent| {
            seen = Some((universe, place, bytes.to_vec(), sent));
            Ok(12)
        });
        assert_eq!(result, Ok(12));
        assert_eq!(
            seen,
            Some((
                TARGET.universe_id,
                TARGET.place_id,
                b"<roblox!".to_vec(),
                mode
            ))
        );
    }
}

#[test]
fn a_refused_upload_says_why_in_roblox_terms_and_keeps_the_raw_error() {
    let result = upload_with(TARGET, b"", PublishMode::Published, |_, _, _, _| {
        Err(CloudError::Http {
            status: 403,
            url: "https://apis.roblox.com/x".to_string(),
        })
    });
    let failure = result.unwrap_err();
    assert!(failure.unchanged, "a 4xx is a definite refusal");
    let message = failure.message;
    assert!(message.starts_with("Publishing isn\u{2019}t allowed on this place."));
    assert!(message.contains("HTTP 403"));

    let network = upload_with(TARGET, b"", PublishMode::Saved, |_, _, _, _| {
        Err(CloudError::Transport("connection refused".to_string()))
    });
    let network = network.unwrap_err();
    assert_eq!(network.message, "network error: connection refused");
    assert!(
        !network.unchanged,
        "a dropped connection may follow an upload that landed"
    );
    assert!(describe(&CloudError::NoApiKey).contains("No Open Cloud API key"));
}

#[test]
fn success_is_output_and_failure_is_an_error_row() {
    assert_eq!(
        outcome(TARGET, PublishMode::Published, &Ok(7)),
        Feedback::Output("Published to Roblox as version 7 of place 17675488706".to_string())
    );
    assert_eq!(
        outcome(TARGET, PublishMode::Saved, &Ok(3)),
        Feedback::Output("Saved to Roblox as version 3 of place 17675488706".to_string())
    );
    assert!(matches!(
        outcome(TARGET, PublishMode::Saved, &Err(Failure::before_sending("nope".to_string()))),
        Feedback::Error(message) if message == "Saving to Roblox failed for place 17675488706: nope"
    ));
}

#[test]
fn the_capture_mock_answers_each_case() {
    assert_eq!(mocked("ok").unwrap(), 7);
    assert!(matches!(
        mocked("401"),
        Err(CloudError::Http { status: 401, .. })
    ));
    assert!(matches!(mocked("network"), Err(CloudError::Transport(_))));
}

/// What `open_roblox_link` puts up, then what `confirm_roblox_link` does to
/// it before the lookup leaves for the network.
fn opened(then: Option<PublishMode>) -> Option<Dialog> {
    Some(Dialog::Link {
        then,
        error: None,
        resolving: None,
    })
}

fn confirm(dialog: &mut Option<Dialog>, token: u64) {
    let Some(Dialog::Link { resolving, .. }) = dialog else {
        panic!("no link dialog to confirm");
    };
    *resolving = Some(token);
}

fn resolving(dialog: &Option<Dialog>) -> Option<u64> {
    match dialog {
        Some(Dialog::Link { resolving, .. }) => *resolving,
        _ => None,
    }
}

#[test]
fn a_lookup_that_lands_after_cancel_neither_links_nor_publishes() {
    // "Link and publish", then Cancel while it says "Looking up place…".
    let mut dialog = opened(Some(PublishMode::Published));
    confirm(&mut dialog, 1);
    assert!(
        dialog.take().is_some(),
        "Cancel closes it, as close_roblox_dialog does"
    );
    assert!(lookup_finished(&mut dialog, 1, Ok(TARGET)).is_none());
    assert!(dialog.is_none(), "a cancelled dialog stays closed");
}

#[test]
fn only_the_lookup_the_reopened_dialog_waits_on_links_and_publishes() {
    // Confirm, Cancel, reopen, confirm again: two lookups in flight.
    let mut dialog = opened(Some(PublishMode::Published));
    confirm(&mut dialog, 1);
    assert!(dialog.take().is_some());
    dialog = opened(Some(PublishMode::Saved));
    confirm(&mut dialog, 2);

    // The first one lands — success or failure — and changes nothing.
    assert!(lookup_finished(&mut dialog, 1, Ok(TARGET)).is_none());
    assert!(lookup_finished::<Target>(&mut dialog, 1, Err("boom".to_string())).is_none());
    assert_eq!(resolving(&dialog), Some(2));
    assert!(matches!(&dialog, Some(Dialog::Link { error: None, .. })));

    // The second one is the one acted on, with the reopened dialog's mode.
    assert_eq!(
        lookup_finished(&mut dialog, 2, Ok(TARGET)),
        Some((TARGET, Some(PublishMode::Saved)))
    );
    assert!(dialog.is_none());
    // And a late duplicate of it can't run a second upload.
    assert!(lookup_finished(&mut dialog, 2, Ok(TARGET)).is_none());
}

#[test]
fn a_failed_lookup_shows_its_reason_and_lets_the_user_confirm_again() {
    let mut dialog = opened(None);
    confirm(&mut dialog, 3);
    assert!(
        lookup_finished::<Target>(&mut dialog, 3, Err("No place with ID 9.".to_string())).is_none()
    );
    assert!(matches!(
        &dialog,
        Some(Dialog::Link { error: Some(e), resolving: None, .. }) if e == "No place with ID 9."
    ));
}

#[test]
fn only_a_refusal_claims_the_place_was_not_changed() {
    let http = |status| CloudError::Http {
        status,
        url: String::new(),
    };
    assert!(refused(&http(401)));
    assert!(refused(&CloudError::RateLimited { retry_after: None }));
    assert!(refused(&CloudError::NoApiKey));
    assert!(!refused(&http(504)));
    assert!(!refused(&CloudError::Transport("timed out".to_string())));
    let unreadable = serde_json::from_str::<u64>("<html>").unwrap_err();
    assert!(!refused(&CloudError::Json(unreadable)));
}

#[test]
fn a_place_with_unions_warns_that_publish_leaves_them_alone() {
    let mut dom = rbx_dom::WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Model", Some(workspace));
    dom.new_instance("Part", "Part", Some(model));
    assert_eq!(not_updated(&dom), None);

    dom.new_instance("UnionOperation", "Union", Some(model));
    dom.new_instance("SurfaceAppearance", "Look", Some(workspace));
    dom.new_instance("UnionOperation", "Union", Some(workspace));
    let warning = not_updated(&dom).unwrap();
    assert!(
        warning.contains("SurfaceAppearance, UnionOperation instances"),
        "{warning}"
    );
}
