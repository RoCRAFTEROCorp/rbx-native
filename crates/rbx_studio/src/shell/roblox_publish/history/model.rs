//! What the Version History window lists, kept apart from GPUI so the
//! paging rules are testable: pages append in order, a reload or a filter
//! change throws away whatever was still on its way, and a user id is shown
//! by name once one is known.

use std::collections::HashMap;

use rbx_cloud::{PlaceVersion, VersionPage};

#[derive(Debug, Default)]
pub(super) struct History {
    pub(super) versions: Vec<PlaceVersion>,
    /// Where the next page starts; `None` once the last page is in.
    pub(super) next_cursor: Option<String>,
    /// Everyone who ever saved the place, for the filter pills.
    pub(super) contributors: Vec<u64>,
    /// Show only the versions this user worked on.
    pub(super) filter: Option<u64>,
    names: HashMap<u64, String>,
    /// Bumped by every reset; a page fetched for an older one is dropped.
    generation: u64,
}

impl History {
    /// Starts over from the first page, keeping the names already known.
    /// Returns the generation the new first page must carry.
    pub(super) fn reset(&mut self, filter: Option<u64>) -> u64 {
        self.generation += 1;
        self.versions.clear();
        self.next_cursor = None;
        self.filter = filter;
        self.generation
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    /// Takes a page fetched for `generation`, or ignores it when a reset
    /// happened since. A version already listed is not listed twice (a
    /// save landing between two page requests shifts the pages by one).
    pub(super) fn accept(&mut self, generation: u64, page: VersionPage) -> bool {
        if generation != self.generation {
            return false;
        }
        for version in page.versions {
            if !self.versions.iter().any(|v| v.version == version.version) {
                self.versions.push(version);
            }
        }
        self.next_cursor = page.next_cursor;
        true
    }

    pub(super) fn learn_names(&mut self, names: impl IntoIterator<Item = (u64, String)>) {
        self.names.extend(names);
    }

    pub(super) fn known_names(&self) -> Vec<u64> {
        self.names.keys().copied().collect()
    }

    pub(super) fn name(&self, user: u64) -> String {
        self.names
            .get(&user)
            .cloned()
            .unwrap_or_else(|| format!("User {user}"))
    }

    /// Who saved `version`: Roblox's `createdBy`, else the first person in
    /// its session. Versions older than Roblox's version history carry
    /// neither.
    pub(super) fn author(&self, version: &PlaceVersion) -> Option<String> {
        version
            .created_by
            .or_else(|| version.contributors.first().copied())
            .map(|user| self.name(user))
    }

    /// The newest version of the place, which there is no point restoring:
    /// the top of the unfiltered list.
    pub(super) fn latest(&self) -> Option<u64> {
        match self.filter {
            None => self.versions.first().map(|v| v.version),
            Some(_) => None,
        }
    }
}

/// The ids in `page` (authors and session members) not yet named, once each.
pub(super) fn unnamed(page: &VersionPage, contributors: &[u64], known: &[u64]) -> Vec<u64> {
    let mut ids: Vec<u64> = page
        .versions
        .iter()
        .flat_map(|v| {
            v.created_by
                .into_iter()
                .chain(v.contributors.iter().copied())
        })
        .chain(contributors.iter().copied())
        .filter(|id| !known.contains(id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// `2026-01-14T04:55:21.168Z` as `2026-01-14 04:55 UTC`. Roblox always
/// answers in UTC; anything not shaped like that is shown as it came.
pub(super) fn date_label(rfc3339: &str) -> String {
    let shaped = rfc3339.len() >= 16
        && rfc3339.as_bytes()[10] == b'T'
        && rfc3339.as_bytes()[13] == b':'
        && rfc3339.ends_with('Z');
    if shaped {
        format!("{} {} UTC", &rfc3339[..10], &rfc3339[11..16])
    } else {
        rfc3339.to_string()
    }
}
