//! The window's network half, all blocking and run off the UI thread:
//! listing a page (with the names it mentions), opening a version as a
//! local copy, and restoring one.
//!
//! `RBX_STUDIO_VERSIONS_MOCK=fixture` answers the listing with two canned
//! pages and a restore with `RBX_STUDIO_PUBLISH_MOCK`'s answer (`ok` when
//! unset) instead of Roblox; `=<HTTP status>` fails the listing with it.
//! Both are for scripted captures and never touch the network.

use std::path::{Path, PathBuf};

use rbx_cloud::{ApiKey, Client, CloudError, PlaceVersion, PublishMode, VersionPage};
use rbx_dom::WeakDom;

use super::super::upload::{self, describe};
use super::super::{Failure, Target, MOCK_VARIABLE as PUBLISH_MOCK};
use super::model::unnamed;

const MOCK_VARIABLE: &str = "RBX_STUDIO_VERSIONS_MOCK";

/// One fetched page and what came with it.
pub(super) struct Loaded {
    pub(super) page: VersionPage,
    /// Fetched with the first unfiltered page only; `None` otherwise.
    pub(super) contributors: Option<Vec<u64>>,
    pub(super) names: Vec<(u64, String)>,
}

pub(super) struct Restored {
    pub(super) version: u64,
    /// [`upload::not_updated`] for the restored file.
    pub(super) warning: Option<String>,
}

fn mock() -> Option<String> {
    std::env::var(MOCK_VARIABLE).ok()
}

fn client() -> Result<Client, String> {
    ApiKey::from_env_or_config()
        .map(|key| Client::new(Some(key)))
        .ok_or_else(|| describe(&CloudError::NoApiKey))
}

/// A refusal of a read, naming the scope it needed — the wizard lists
/// `universe.place:read` as optional, so a key without it is common.
pub(super) fn describe_read(err: &CloudError, scope: &str) -> String {
    match err {
        CloudError::Http {
            status: 401 | 403, ..
        } => format!(
            "The API key can\u{2019}t read this place: it needs {scope} on this experience \u{2014} Home \u{203a} Manage key. ({err})"
        ),
        CloudError::Http { status: 404, .. } => {
            format!("Roblox has no such place or version. ({err})")
        }
        _ => describe(err),
    }
}

pub(super) fn load(
    place_id: u64,
    cursor: Option<String>,
    filter: Option<u64>,
    known: Vec<u64>,
) -> Result<Loaded, String> {
    if let Some(which) = mock() {
        return mocked_load(&which, cursor.as_deref(), filter);
    }
    let client = client()?;
    let read = |err: CloudError| describe_read(&err, "universe.place:read");
    let first_unfiltered = cursor.is_none() && filter.is_none();
    let page = client
        .place_versions(place_id, cursor.as_deref(), filter)
        .map_err(read)?;
    let contributors = if first_unfiltered {
        Some(client.place_contributors(place_id).map_err(read)?)
    } else {
        None
    };
    let ids = unnamed(&page, contributors.as_deref().unwrap_or(&[]), &known);
    // A name is a nicety: a failed lookup shows "User <id>" instead.
    let names = client.user_display_names(&ids).unwrap_or_default();
    Ok(Loaded {
        page,
        contributors,
        names,
    })
}

/// Downloads `version` beside Home's local copies and opens it in a new
/// editor process, as Studio's Open Local Copy opens a new session. The copy
/// is unlinked: saving it back to Roblox goes through the game picker, so it
/// can't overwrite the place by accident.
pub(super) fn open_copy(place_id: u64, version: u64) -> Result<PathBuf, String> {
    let bytes = client()?
        .download_place_version(place_id, version)
        .map_err(|err| describe_read(&err, "legacy-asset:manage"))?;
    let dir = crate::home::places_dir()
        .ok_or("no config directory to download into")?
        .join("versions");
    let ext = match crate::save::Format::sniff(&bytes) {
        crate::save::Format::Binary => "rbxl",
        crate::save::Format::Xml => "rbxlx",
    };
    let path = copy_path(&dir, place_id, version, ext);
    crate::settings::write_atomic(&path, &bytes).map_err(|err| err.to_string())?;
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let mut editor = std::process::Command::new(exe);
    // A scripted capture's `RBX_STUDIO_*` asks this editor to act at open;
    // the copy is a fresh session and must not repeat it.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("RBX_STUDIO_") {
            editor.env_remove(name);
        }
    }
    editor
        .arg(&path)
        .spawn()
        .map_err(|err| format!("the editor couldn\u{2019}t be started: {err}"))?;
    Ok(path)
}

/// `<place>-v<version>.<ext>`, or the first free `… N` beside it: an
/// earlier copy may hold the user's edits.
pub(super) fn copy_path(dir: &Path, place_id: u64, version: u64, ext: &str) -> PathBuf {
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{place_id}-v{version}.{ext}")),
            n => dir.join(format!("{place_id}-v{version} {n}.{ext}")),
        })
        .find(|path| !path.exists())
        .expect("an unbounded range always finds a free name")
}

pub(super) fn restore(
    target: Target,
    version: u64,
    mode: PublishMode,
) -> Result<Restored, Failure> {
    if mock().is_some() {
        let answer = std::env::var(PUBLISH_MOCK).unwrap_or_else(|_| "ok".to_string());
        return restore_with(
            target,
            version,
            mode,
            |_, _| Ok(b"<roblox!".to_vec()),
            |_, _, _, _| upload::mocked(&answer),
        );
    }
    let client = client().map_err(Failure::before_sending)?;
    restore_with(
        target,
        version,
        mode,
        |place, version| client.download_place_version(place, version),
        |universe, place, bytes, mode| client.publish_place(universe, place, bytes, mode),
    )
}

/// Roblox's restore, done the only way Open Cloud allows: the old version's
/// file uploaded as a new version. The seam the tests drive.
pub(super) fn restore_with(
    target: Target,
    version: u64,
    mode: PublishMode,
    download: impl FnOnce(u64, u64) -> Result<Vec<u8>, CloudError>,
    publish: impl FnOnce(u64, u64, &[u8], PublishMode) -> Result<u64, CloudError>,
) -> Result<Restored, Failure> {
    let bytes = download(target.place_id, version).map_err(|err| {
        Failure::before_sending(format!(
            "Version {version} couldn\u{2019}t be downloaded: {}",
            describe_read(&err, "legacy-asset:manage")
        ))
    })?;
    let warning = parse(&bytes).and_then(|dom| upload::not_updated(&dom));
    let version = upload::upload_with(target, &bytes, mode, publish)?;
    Ok(Restored { version, warning })
}

/// Only for the warning, so a file that won't parse simply has none:
/// Roblox made it, and it is uploaded as it came.
fn parse(bytes: &[u8]) -> Option<WeakDom> {
    if rbx_xml::is_xml(bytes) {
        rbx_xml::deserialize(std::str::from_utf8(bytes).ok()?).ok()
    } else {
        rbx_binary::deserialize(bytes).ok()
    }
}

fn mocked_load(which: &str, cursor: Option<&str>, filter: Option<u64>) -> Result<Loaded, String> {
    if let Ok(status) = which.parse::<u16>() {
        let err = CloudError::Http {
            status,
            url: "https://apis.roblox.com/place-version-history-api/v1/…/history".to_string(),
        };
        return Err(describe_read(&err, "universe.place:read"));
    }
    let version =
        |n: u64, by: Option<u64>, published: bool, title: Option<&str>, at: &str| PlaceVersion {
            version: n,
            title: title.map(str::to_string),
            description: None,
            contributors: by.into_iter().collect(),
            created_by: by,
            is_published: published,
            created_time: at.to_string(),
        };
    let (first, second) = (Some(925308243), Some(156));
    let (versions, next) = match cursor {
        None => (
            vec![
                version(214, first, false, None, "2026-10-04T17:25:31.120Z"),
                version(
                    213,
                    second,
                    true,
                    Some("Lobby lighting pass"),
                    "2026-10-03T21:02:11.004Z",
                ),
                version(212, second, false, None, "2026-10-03T20:41:57.630Z"),
                version(211, first, false, None, "2026-10-02T09:13:40.981Z"),
                version(
                    210,
                    first,
                    true,
                    Some("Spring event"),
                    "2026-09-28T18:00:02.442Z",
                ),
                version(209, None, false, None, "2025-06-11T16:23:16.567Z"),
            ],
            Some("version_fixture".to_string()),
        ),
        Some(_) => (
            vec![version(208, None, false, None, "2025-03-31T20:26:37.903Z")],
            None,
        ),
    };
    let versions = versions
        .into_iter()
        .filter(|v| filter.is_none_or(|user| v.contributors.contains(&user)))
        .collect();
    Ok(Loaded {
        page: VersionPage {
            versions,
            next_cursor: next,
        },
        contributors: (cursor.is_none() && filter.is_none()).then(|| vec![925308243, 156]),
        names: vec![(925308243, "Cheeteau".into()), (156, "builderman".into())],
    })
}
