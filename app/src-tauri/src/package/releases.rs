// SPDX-License-Identifier: GPL-3.0-only
use crate::github::{self, Release};

/// The newest published firmware release and where its package manifest lives.
pub struct FirmwareRelease {
    pub manifest_url: String,
    pub release_url: String,
    pub notes: Option<String>,
}

fn newest_manifest(releases: Vec<Release>, target: &str) -> Result<FirmwareRelease, String> {
    let release = releases
        .into_iter()
        .filter(|release| !release.draft && release.published_at.is_some())
        .max_by(|a, b| a.published_at.cmp(&b.published_at))
        .ok_or("No RevolaOS release has been published yet. Choose a local package to continue.")?;
    let name = format!("{target}-manifest.json");
    let notes = github::notes(&release);
    let manifest_url = release
        .assets
        .into_iter()
        .find(|asset| asset.name == name)
        .map(|asset| asset.browser_download_url)
        .ok_or("The latest RevolaOS release doesn’t include a compatible firmware package. Choose a local package to continue.")?;
    Ok(FirmwareRelease {
        manifest_url,
        release_url: release.html_url,
        notes,
    })
}

pub(super) fn latest(target: &str) -> Result<FirmwareRelease, String> {
    newest_manifest(github::releases("RevolaOS/revola")?, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn newest_published_release_includes_prereleases_and_skips_drafts() {
        let release = |tag: &str, date: &str, draft: bool, prerelease: bool| {
            json!({
                "html_url": format!("https://github.com/RevolaOS/revola/releases/tag/{tag}"),
                "body": format!("Notes for {tag}\n"),
                "draft": draft,
                "prerelease": prerelease,
                "published_at": date,
                "assets": [{
                    "name": "classic-manifest.json",
                    "browser_download_url": format!("https://github.com/RevolaOS/revola/releases/download/{tag}/classic-manifest.json")
                }]
            })
        };
        let releases = serde_json::from_value(json!([
            release("v0.2.0", "2026-09-28T00:00:00Z", true, false),
            release("v0.1.0", "2026-09-25T00:00:00Z", false, false),
            release("v0.2.0-alpha.1", "2026-09-26T00:00:00Z", false, true)
        ]))
        .unwrap();
        let newest = newest_manifest(releases, "classic").unwrap();
        assert_eq!(
            newest.manifest_url,
            "https://github.com/RevolaOS/revola/releases/download/v0.2.0-alpha.1/classic-manifest.json"
        );
        assert_eq!(
            newest.release_url,
            "https://github.com/RevolaOS/revola/releases/tag/v0.2.0-alpha.1"
        );
        assert_eq!(newest.notes.as_deref(), Some("Notes for v0.2.0-alpha.1"));
        assert!(newest_manifest(Vec::new(), "classic").is_err());
    }
    #[test]
    fn does_not_fall_back_to_old_firmware_packages_or_releases() {
        let release = |date, names: &[&str]| {
            json!({
                "html_url": "https://example.com", "body": null,
                "draft": false, "published_at": date,
                "assets": names.iter().map(|name| json!({
                    "name": name, "browser_download_url": format!("https://example.com/{name}")
                })).collect::<Vec<_>>()
            })
        };
        let releases = serde_json::from_value(json!([
            release("2026-10-05", &["classic-manifest.json"]),
            release(
                "2026-10-06",
                &["classic7g-2.0.4-manifest.json", "manifest.json"]
            )
        ]))
        .unwrap();
        assert!(newest_manifest(releases, "classic").is_err());
    }
}
