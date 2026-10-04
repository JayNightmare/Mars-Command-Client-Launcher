use serde::{Deserialize, Serialize};
use std::time::Duration;

const RELEASES_URL: &str =
    "https://api.github.com/repos/JayNightmare/Mars-Command-Client-Launcher/releases?per_page=100";
const INSTALLED_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SetupVersion(u64, u64, u64);

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    html_url: String,
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug)]
struct ReleaseCandidate {
    version: SetupVersion,
    asset_url: String,
    release_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationRepairStatus {
    pub installed_version: String,
    pub latest_version: Option<String>,
    pub asset_url: Option<String>,
    pub release_url: Option<String>,
    pub update_available: bool,
    pub platform: String,
    pub message: Option<String>,
}

fn parse_version(version: &str) -> Option<SetupVersion> {
    let version = version
        .strip_prefix('v')
        .or_else(|| version.strip_prefix('V'))
        .unwrap_or(version);
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(SetupVersion(major, minor, patch))
}

fn update_available(installed: SetupVersion, latest: SetupVersion) -> bool {
    latest > installed
}

fn parse_setup_asset_version(name: &str, expected_extension: &str) -> Option<SetupVersion> {
    let basename = name
        .rsplit(|character| character == '/' || character == '\\')
        .next()?;
    let (stem, extension) = basename.rsplit_once('.')?;
    if !extension.eq_ignore_ascii_case(expected_extension) {
        return None;
    }
    let version = stem
        .get(..6)
        .filter(|prefix| prefix.eq_ignore_ascii_case("setup-"))
        .and_then(|_| stem.get(6..))?;
    parse_version(version)
}

fn latest_compatible_candidate(
    releases: &[GitHubRelease],
    expected_extension: &str,
) -> Option<ReleaseCandidate> {
    releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .flat_map(|release| {
            release.assets.iter().filter_map(move |asset| {
                let version =
                    parse_setup_asset_version(&asset.name, expected_extension).or_else(|| {
                        let generic_name = format!("setup.{expected_extension}");
                        asset
                            .name
                            .eq_ignore_ascii_case(&generic_name)
                            .then(|| parse_version(&release.tag_name))
                            .flatten()
                    })?;
                if !asset.browser_download_url.starts_with("https://")
                    || !release.html_url.starts_with("https://")
                {
                    return None;
                }
                Some(ReleaseCandidate {
                    version,
                    asset_url: asset.browser_download_url.clone(),
                    release_url: release.html_url.clone(),
                })
            })
        })
        .max_by_key(|candidate| candidate.version)
}

#[cfg(target_os = "windows")]
fn supported_platform() -> Option<(&'static str, &'static str)> {
    Some(("Windows", "exe"))
}

#[cfg(target_os = "linux")]
fn supported_platform() -> Option<(&'static str, &'static str)> {
    Some(("Linux", "deb"))
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn supported_platform() -> Option<(&'static str, &'static str)> {
    None
}

pub async fn check() -> Result<InstallationRepairStatus, String> {
    let installed = parse_version(INSTALLED_VERSION)
        .ok_or_else(|| "The installed build version is not in X.X.X format".to_string())?;
    let Some((platform, extension)) = supported_platform() else {
        return Ok(InstallationRepairStatus {
            installed_version: INSTALLED_VERSION.to_string(),
            latest_version: None,
            asset_url: None,
            release_url: None,
            update_available: false,
            platform: std::env::consts::OS.to_string(),
            message: Some(
                "Installer repair checks are available only for Windows (.exe) and Linux (.deb) builds.".into(),
            ),
        });
    };

    let client = reqwest::Client::builder()
        .user_agent(concat!("MarsCommand/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| format!("Could not create release checker: {error}"))?;
    let releases = client
        .get(RELEASES_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("Could not check GitHub releases: {error}"))?
        .error_for_status()
        .map_err(|error| format!("GitHub releases returned an error: {error}"))?
        .json::<Vec<GitHubRelease>>()
        .await
        .map_err(|error| format!("Could not parse GitHub releases: {error}"))?;

    let Some(candidate) = latest_compatible_candidate(&releases, extension) else {
        return Ok(InstallationRepairStatus {
            installed_version: INSTALLED_VERSION.to_string(),
            latest_version: None,
            asset_url: None,
            release_url: None,
            update_available: false,
            platform: platform.to_string(),
            message: Some(format!(
                "No compatible setup-{0}.{1} asset was found in public releases.",
                "X.X.X", extension
            )),
        });
    };

    let update_available = update_available(installed, candidate.version);
    Ok(InstallationRepairStatus {
        installed_version: INSTALLED_VERSION.to_string(),
        latest_version: Some(format_version(candidate.version)),
        asset_url: Some(candidate.asset_url),
        release_url: Some(candidate.release_url),
        update_available,
        platform: platform.to_string(),
        message: None,
    })
}

fn format_version(version: SetupVersion) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}

#[cfg(test)]
mod tests {
    use super::{
        latest_compatible_candidate, parse_setup_asset_version, parse_version, GitHubAsset,
        GitHubRelease,
    };

    fn release(asset_name: &str, version: &str) -> GitHubRelease {
        GitHubRelease {
            html_url: format!("https://github.com/example/releases/tag/{version}"),
            tag_name: format!("v{version}"),
            draft: false,
            prerelease: false,
            assets: vec![GitHubAsset {
                name: asset_name.into(),
                browser_download_url: format!("https://example.com/{asset_name}"),
            }],
        }
    }

    #[test]
    fn parses_versioned_setup_assets_for_supported_platforms() {
        assert_eq!(
            parse_setup_asset_version("setup-1.2.3.exe", "exe"),
            Some(parse_version("1.2.3").unwrap())
        );
        assert_eq!(
            parse_setup_asset_version("setup-2.10.4.deb", "deb"),
            Some(parse_version("2.10.4").unwrap())
        );
        assert_eq!(
            parse_setup_asset_version("SETUP-1.2.3.EXE", "exe"),
            Some(parse_version("1.2.3").unwrap())
        );
    }

    #[test]
    fn uses_the_release_tag_for_legacy_generic_setup_assets() {
        let mut generic = release("setup.exe", "1.2.4");
        generic.tag_name = "v1.2.4".into();
        let candidate = latest_compatible_candidate(&[generic], "exe").unwrap();

        assert_eq!(candidate.version, parse_version("1.2.4").unwrap());
        assert!(candidate.asset_url.ends_with("setup.exe"));
    }

    #[test]
    fn rejects_malformed_or_incompatible_setup_assets() {
        assert_eq!(parse_setup_asset_version("setup-1.2.exe", "exe"), None);
        assert_eq!(parse_setup_asset_version("setup-1.2.3.zip", "exe"), None);
        assert_eq!(parse_setup_asset_version("client-1.2.3.exe", "exe"), None);
        assert_eq!(parse_setup_asset_version("setup-1.2.3.4.deb", "deb"), None);
    }

    #[test]
    fn selects_the_highest_stable_release_with_a_compatible_asset() {
        let releases = vec![
            release("setup-1.8.0.exe", "1.8.0"),
            release("setup-2.0.0.deb", "2.0.0"),
            GitHubRelease {
                prerelease: true,
                ..release("setup-9.0.0.exe", "9.0.0")
            },
            GitHubRelease {
                draft: true,
                ..release("setup-10.0.0.exe", "10.0.0")
            },
        ];

        let candidate = latest_compatible_candidate(&releases, "exe").unwrap();
        assert_eq!(candidate.version, parse_version("1.8.0").unwrap());
        assert!(candidate.asset_url.ends_with("setup-1.8.0.exe"));
    }

    #[test]
    fn repair_is_available_only_when_the_compatible_release_is_newer() {
        let installed = parse_version("1.2.3").unwrap();
        assert!(super::update_available(
            installed,
            parse_version("1.10.0").unwrap()
        ));
        assert!(!super::update_available(
            installed,
            parse_version("1.2.3").unwrap()
        ));
        assert!(!super::update_available(
            installed,
            parse_version("1.1.9").unwrap()
        ));
    }
}
