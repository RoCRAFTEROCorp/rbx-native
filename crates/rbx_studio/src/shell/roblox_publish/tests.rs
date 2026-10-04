use std::path::{Path, PathBuf};

use rbx_cloud::{CloudError, PublishMode};

use super::{describe, linked_target, mocked, outcome, upload_with, Target};
use crate::command_bar::Feedback;
use crate::home::RecentPlace;

const TARGET: Target = Target {
    universe_id: 6053515322,
    place_id: 17675488706,
};

fn recent(path: &str, universe_id: Option<u64>, place_id: Option<u64>) -> RecentPlace {
    RecentPlace {
        path: PathBuf::from(path),
        universe_id,
        place_id,
        name: None,
        opened: None,
    }
}

#[test]
fn a_file_is_linked_only_when_its_recent_entry_has_both_ids() {
    let list = [
        recent("/places/a.rbxl", Some(1), Some(2)),
        recent("/places/b.rbxl", None, None),
        recent("/places/c.rbxl", None, Some(5)),
    ];
    assert_eq!(
        linked_target(&list, Path::new("/places/a.rbxl")),
        Some(Target {
            universe_id: 1,
            place_id: 2
        })
    );
    assert_eq!(linked_target(&list, Path::new("/places/b.rbxl")), None);
    assert_eq!(linked_target(&list, Path::new("/places/c.rbxl")), None);
    assert_eq!(linked_target(&list, Path::new("/places/d.rbxl")), None);
}

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
    let message = result.unwrap_err();
    assert!(message.starts_with("Publishing isn\u{2019}t allowed on this place."));
    assert!(message.contains("HTTP 403"));

    let network = upload_with(TARGET, b"", PublishMode::Saved, |_, _, _, _| {
        Err(CloudError::Transport("connection refused".to_string()))
    });
    assert_eq!(
        network.unwrap_err(),
        "network error: connection refused".to_string()
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
        outcome(TARGET, PublishMode::Saved, &Err("nope".to_string())),
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
