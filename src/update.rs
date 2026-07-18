//! Self-updater, modeled on jcode's versioned-store design:
//!
//! ```text
//! ~/.local/bin/bang                       symlink ->
//! ~/.local/share/bang/current/bang        channel symlink ->
//! ~/.local/share/bang/versions/<v>/bang   actual binary
//! ```
//!
//! Properties this buys:
//! - atomic upgrade: install into `versions/<v>/`, then flip one symlink
//! - instant rollback: re-point `current` at any older version dir
//! - never overwrites a running binary
//!
//! The update check authenticates to the GitHub API when `GH_TOKEN`/
//! `GITHUB_TOKEN` or a `gh` login is available, avoiding the shared
//! 60 req/hour per-IP bucket that causes spurious 403s. All check
//! failures are silent: a search must never break because GitHub is
//! unreachable.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const REPO: &str = "hongnoul/bang";
const CHECK_INTERVAL_SECS: u64 = 60 * 60 * 24;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn run(check_only: bool) -> Result<()> {
    let release = fetch_latest_release()?;
    let latest = release.tag_name.trim_start_matches('v').to_string();
    let current = env!("CARGO_PKG_VERSION");

    if !is_newer(&latest, current) {
        println!("bang {current} is up to date");
        return Ok(());
    }
    println!("update available: {current} -> {latest}");
    if check_only {
        return Ok(());
    }

    let asset_name = platform_asset();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| anyhow!("no release asset for this platform ({asset_name})"))?;
    let checksum_asset = release
        .assets
        .iter()
        .find(|a| a.name == format!("{asset_name}.sha256"));

    let store = store_dir()?;
    let version_dir = store.join("versions").join(&latest);
    std::fs::create_dir_all(&version_dir)?;
    let staged = version_dir.join("bang.partial");
    let final_bin = version_dir.join("bang");

    println!("downloading {}...", asset.name);
    download(&asset.browser_download_url, &staged)?;

    if let Some(sum_asset) = checksum_asset {
        let expected = ureq::get(&sum_asset.browser_download_url)
            .call()?
            .into_string()?;
        let expected = expected.split_whitespace().next().unwrap_or_default();
        let actual = sha256_file(&staged)?;
        if !expected.eq_ignore_ascii_case(&actual) {
            std::fs::remove_file(&staged).ok();
            anyhow::bail!("checksum mismatch: expected {expected}, got {actual}");
        }
        println!("checksum verified");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&staged, &final_bin)?;

    // Atomic channel flip: symlink a temp name, then rename over `current`.
    let current_link = store.join("current");
    #[cfg(unix)]
    {
        let tmp_link = store.join(".current.tmp");
        std::fs::remove_file(&tmp_link).ok();
        std::os::unix::fs::symlink(&version_dir, &tmp_link)?;
        std::fs::rename(&tmp_link, &current_link)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::remove_dir_all(&current_link).ok();
        std::fs::create_dir_all(&current_link)?;
        std::fs::copy(&final_bin, current_link.join("bang"))?;
    }

    record_check_time();
    println!("updated to {latest} (takes effect on next run)");
    println!(
        "rollback: ln -sfn {}/versions/<version> {}/current",
        store.display(),
        store.display()
    );
    Ok(())
}

/// Print a one-line notice after successful searches, at most once per day,
/// and never let any failure surface.
pub fn maybe_print_update_notice() {
    let Ok(store) = store_dir() else { return };
    let stamp = store.join("last-check");
    let fresh = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < CHECK_INTERVAL_SECS);
    if fresh {
        return;
    }
    record_check_time();
    if let Ok(release) = fetch_latest_release() {
        let latest = release.tag_name.trim_start_matches('v');
        if is_newer(latest, env!("CARGO_PKG_VERSION")) {
            eprintln!("(bang {latest} is available; run `bang update`)");
        }
    }
}

fn fetch_latest_release() -> Result<Release> {
    let mut request = ureq::get(&format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    ))
    .set("Accept", "application/vnd.github+json")
    .set("User-Agent", "bang-updater")
    .timeout(std::time::Duration::from_secs(5));
    if let Some(token) = github_token() {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    Ok(request
        .call()
        .context("release check failed")?
        .into_json()?)
}

/// GH_TOKEN/GITHUB_TOKEN env vars, then `gh auth token`. Avoids the shared
/// unauthenticated per-IP rate limit bucket.
fn github_token() -> Option<String> {
    for name in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(token) = std::env::var(name) {
            let token = token.trim().to_string();
            if !token.is_empty() {
                return Some(token);
            }
        }
    }
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

fn download(url: &str, dest: &PathBuf) -> Result<()> {
    let response = ureq::get(url).call()?;
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(dest)?;
    std::io::copy(&mut reader, &mut file)?;
    Ok(())
}

fn sha256_file(path: &PathBuf) -> Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn store_dir() -> Result<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| anyhow!("no data dir"))?
        .join("bang");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn record_check_time() {
    if let Ok(store) = store_dir() {
        let _ = std::fs::write(store.join("last-check"), b"");
    }
}

fn platform_asset() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    };
    let arch = std::env::consts::ARCH;
    format!("bang-{os}-{arch}")
}

/// Lexicographic-free semver comparison (major.minor.patch numeric fields).
fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    parse(candidate) > parse(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_is_numeric() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn platform_asset_shape() {
        let asset = platform_asset();
        assert!(asset.starts_with("bang-"));
        assert!(!asset.contains("macos"), "macOS must map to darwin");
    }
}
