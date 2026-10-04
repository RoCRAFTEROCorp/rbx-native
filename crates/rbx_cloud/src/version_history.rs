//! A place's saved and published versions: `place-version-history-api`'s
//! `GET /v1/{placeId}/history` and `/contributors` (`creator-docs`,
//! `reference/cloud/openapi.json`, both scope `universe.place:read`), and
//! one version's place file through the keyed asset-delivery route with
//! `/version/{n}` — what restoring one starts from. Open Cloud has no
//! "revert a place" call: Roblox's own restore makes a new version out of the
//! old one's file (`creator-docs`, `projects/version-history.md`), which is
//! `Client::publish_place` with these bytes.

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};

const HISTORY_URL: &str = "https://apis.roblox.com/place-version-history-api/v1";

/// What the endpoint answers when no `pageSize` is sent (checked live,
/// 2026-10-04); asked for explicitly so a change of default can't shrink it.
const PAGE_SIZE: &str = "50";

/// One saved or published version, newest first in a [`VersionPage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceVersion {
    pub version: u64,
    /// The version name and notes, when someone wrote them.
    pub title: Option<String>,
    pub description: Option<String>,
    /// Everyone in the edit session the version was saved from. Versions
    /// older than Roblox's version history feature list nobody.
    pub contributors: Vec<u64>,
    pub created_by: Option<u64>,
    pub is_published: bool,
    /// RFC 3339, UTC, exactly as Roblox sent it.
    pub created_time: String,
}

/// One page of history. `next_cursor` is `None` on the last page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionPage {
    pub versions: Vec<PlaceVersion>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryRaw {
    next_cursor: Option<String>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    place_versions: Option<Vec<PlaceVersionRaw>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaceVersionRaw {
    // A string in the schema ("410"), though it is always a number.
    version: Option<String>,
    title: Option<String>,
    description: Option<String>,
    #[serde(default)]
    contributors: Option<Vec<u64>>,
    created_by: Option<u64>,
    #[serde(default)]
    is_published: bool,
    #[serde(default)]
    created_time: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContributorsRaw {
    next_cursor: Option<String>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    contributors: Option<Vec<u64>>,
}

/// The cursor to ask for next. `hasMore` decides, not `nextCursor`: the
/// contributors endpoint hands back a cursor on its last page too (seen
/// live, 2026-10-04), and following it would loop.
fn next(has_more: bool, cursor: Option<String>) -> Option<String> {
    cursor.filter(|c| has_more && !c.is_empty())
}

fn parse_history(body: &[u8]) -> Result<VersionPage, CloudError> {
    let raw: HistoryRaw = serde_json::from_slice(body)?;
    let versions = raw
        .place_versions
        .unwrap_or_default()
        .into_iter()
        .map(|v| {
            let version = v
                .version
                .as_deref()
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| {
                    CloudError::UnexpectedShape(format!(
                        "place version number {:?} is not a number",
                        v.version
                    ))
                })?;
            Ok(PlaceVersion {
                version,
                title: v.title.filter(|t| !t.is_empty()),
                description: v.description.filter(|d| !d.is_empty()),
                contributors: v.contributors.unwrap_or_default(),
                created_by: v.created_by,
                is_published: v.is_published,
                created_time: v.created_time,
            })
        })
        .collect::<Result<_, CloudError>>()?;
    Ok(VersionPage {
        versions,
        next_cursor: next(raw.has_more, raw.next_cursor),
    })
}

/// The query a history request sends, in order.
fn history_query(cursor: Option<&str>, contributor: Option<u64>) -> Vec<(&'static str, String)> {
    let mut query = vec![("pageSize", PAGE_SIZE.to_string())];
    if let Some(cursor) = cursor {
        query.push(("cursor", cursor.to_string()));
    }
    if let Some(user) = contributor {
        query.push(("contributor", user.to_string()));
    }
    query
}

impl Client {
    /// One page of `place_id`'s versions, newest first; `contributor` keeps
    /// only the versions that user worked on.
    pub fn place_versions(
        &self,
        place_id: u64,
        cursor: Option<&str>,
        contributor: Option<u64>,
    ) -> Result<VersionPage, CloudError> {
        let url = format!("{HISTORY_URL}/{place_id}/history");
        let query = history_query(cursor, contributor);
        let response = self.get_with(&url, true, true, |mut req| {
            for (name, value) in &query {
                req = req.query(*name, value);
            }
            req
        })?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        parse_history(&response.body)
    }

    /// Every user who ever saved `place_id`, all pages walked.
    pub fn place_contributors(&self, place_id: u64) -> Result<Vec<u64>, CloudError> {
        let url = format!("{HISTORY_URL}/{place_id}/contributors");
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let response = self.get_with(&url, true, true, |req| match &cursor {
                Some(cursor) => req.query("cursor", cursor),
                None => req,
            })?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    &url,
                    response.status,
                    &response.headers,
                ));
            }
            let raw: ContributorsRaw = serde_json::from_slice(&response.body)?;
            all.extend(raw.contributors.unwrap_or_default());
            match next(raw.has_more, raw.next_cursor) {
                Some(more) if Some(&more) != cursor.as_ref() => cursor = Some(more),
                _ => return Ok(all),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Place 17675488706, 2026-10-04, trimmed to three versions.
    const HISTORY: &str = r#"{"nextCursor":"version_2zwAAAZXt4nvPozQwOA","hasMore":true,"placeVersions":[{"version":"410","title":null,"description":null,"contributors":[925308243],"saveType":2,"isPublished":false,"publishStatus":0,"hasNotes":false,"createdTime":"2026-01-14T04:55:21.168Z","createdBy":925308243},{"version":"409","title":"Lobby pass","description":"New spawn","contributors":[],"saveType":0,"isPublished":true,"publishStatus":0,"hasNotes":true,"createdTime":"2025-06-11T16:23:16.567Z","createdBy":null},{"version":"408","title":null,"description":null,"contributors":[],"saveType":0,"isPublished":false,"publishStatus":0,"hasNotes":false,"createdTime":"2025-03-31T20:26:37.903Z","createdBy":null}]}"#;

    #[test]
    fn parses_a_real_history_page() {
        let page = parse_history(HISTORY.as_bytes()).unwrap();
        assert_eq!(
            page.next_cursor.as_deref(),
            Some("version_2zwAAAZXt4nvPozQwOA")
        );
        let numbers: Vec<u64> = page.versions.iter().map(|v| v.version).collect();
        assert_eq!(numbers, [410, 409, 408]);
        let newest = &page.versions[0];
        assert_eq!(newest.created_by, Some(925308243));
        assert_eq!(newest.contributors, [925308243]);
        assert_eq!(newest.created_time, "2026-01-14T04:55:21.168Z");
        assert!(!newest.is_published);
        let noted = &page.versions[1];
        assert_eq!(noted.title.as_deref(), Some("Lobby pass"));
        assert_eq!(noted.description.as_deref(), Some("New spawn"));
        assert!(noted.is_published);
        assert_eq!(noted.created_by, None);
    }

    #[test]
    fn the_last_page_has_no_cursor_even_when_roblox_sends_one() {
        let body = br#"{"nextCursor":"version_x","hasMore":false,"placeVersions":[]}"#;
        assert_eq!(parse_history(body).unwrap().next_cursor, None);
        let empty = br#"{"nextCursor":"","hasMore":true,"placeVersions":null}"#;
        let page = parse_history(empty).unwrap();
        assert_eq!(page.next_cursor, None);
        assert!(page.versions.is_empty());
    }

    #[test]
    fn a_version_that_is_not_a_number_is_an_error_not_a_zero() {
        let body = br#"{"hasMore":false,"placeVersions":[{"version":"latest"}]}"#;
        assert!(matches!(
            parse_history(body),
            Err(CloudError::UnexpectedShape(_))
        ));
    }

    #[test]
    fn parses_the_real_contributors_answer() {
        // Same place, same day: a cursor on a page with nothing after it.
        let raw: ContributorsRaw = serde_json::from_str(
            r#"{"nextCursor":"contributor_2zwAAAZu627uwrlZtOXRrK2ViRTNNPS0x","hasMore":false,"contributors":[925308243]}"#,
        )
        .unwrap();
        assert_eq!(raw.contributors.as_deref(), Some(&[925308243][..]));
        assert_eq!(next(raw.has_more, raw.next_cursor), None);
    }

    #[test]
    fn the_query_carries_page_size_cursor_and_contributor() {
        assert_eq!(history_query(None, None), [("pageSize", "50".to_string())]);
        assert_eq!(
            history_query(Some("version_abc"), Some(7)),
            [
                ("pageSize", "50".to_string()),
                ("cursor", "version_abc".to_string()),
                ("contributor", "7".to_string()),
            ]
        );
    }
}
