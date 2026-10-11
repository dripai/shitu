//! User-initiated portable updates from this project's stable GitHub releases.
mod install;
pub use install::{Launched, helper_exit_code, launch, signal_started, startup_notice};

use anyhow::{Context, Result, bail, ensure};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const API: &str = "https://api.github.com/repos/dripai/shitu/releases/latest";
const RELEASES: &str = "https://github.com/dripai/shitu/releases";
const MAX_ARCHIVE: u64 = 128 * 1024 * 1024;
const MAX_EXE: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
}
#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: Option<String>,
    assets: Vec<Asset>,
}
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    archive: Asset,
    checksum: Asset,
}
impl Release {
    pub fn page_url(&self) -> String {
        format!("{RELEASES}/tag/v{}", self.version)
    }
}
#[derive(Debug)]
pub struct Check {
    pub latest: String,
    pub update: Option<Release>,
    pub portable: bool,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .https_only(true)
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .timeout_write(Duration::from_secs(20))
        .redirects(5)
        .user_agent(concat!("ShiTu/", env!("CARGO_PKG_VERSION")))
        .build()
}
fn bounded_bytes(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "Response exceeds size limit");
    Ok(bytes)
}
pub fn check() -> Result<Check> {
    // No ping/preflight request: one bounded request tests connectivity and checks
    // the actual release. The UI decides whether errors should be visible.
    let response = agent()
        .get(API)
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .timeout(Duration::from_secs(20))
        .call()
        .context("GitHub release check")?;
    let bytes = bounded_bytes(response.into_reader(), 1024 * 1024)?;
    let mut check = parse_release(&bytes, env!("CARGO_PKG_VERSION"))?;
    check.portable = install::is_portable()?;
    Ok(check)
}
fn parse_release(bytes: &[u8], current: &str) -> Result<Check> {
    let response: ReleaseResponse = serde_json::from_slice(bytes)?;
    ensure!(
        !response.draft && !response.prerelease,
        "Expected a stable public release"
    );
    let version = Version::parse(
        response
            .tag_name
            .strip_prefix('v')
            .context("Invalid release tag")?,
    )?;
    ensure!(
        version.pre.is_empty() && version.build.is_empty(),
        "Expected a stable release tag"
    );
    let mut result = Check {
        latest: version.to_string(),
        update: None,
        portable: false,
    };
    if version <= Version::parse(current)? {
        return Ok(result);
    }
    let name = format!("ShiTu-v{version}-windows-x86_64.zip");
    let asset = |name: &str, max: u64| -> Result<Asset> {
        let matching: Vec<_> = response.assets.iter().filter(|a| a.name == name).collect();
        ensure!(
            matching.len() == 1,
            "Missing or duplicate release asset: {name}"
        );
        let asset = matching[0];
        ensure!(
            asset.size > 0 && asset.size <= max,
            "Invalid release asset size: {name}"
        );
        ensure!(
            asset.browser_download_url == format!("{RELEASES}/download/v{version}/{name}"),
            "Unexpected release asset URL"
        );
        Ok(asset.clone())
    };
    result.update = Some(Release {
        version: version.to_string(),
        archive: asset(&name, MAX_ARCHIVE)?,
        checksum: asset(&format!("{name}.sha256"), 4096)?,
        notes: response
            .body
            .unwrap_or_default()
            .chars()
            .take(12000)
            .collect(),
    });
    Ok(result)
}
fn checksum(bytes: &[u8], name: &str) -> Result<String> {
    let text = std::str::from_utf8(bytes)?.trim();
    let fields: Vec<_> = text.split_whitespace().collect();
    ensure!(
        fields.len() == 2 && fields[1].trim_start_matches('*') == name,
        "Checksum filename mismatch"
    );
    ensure!(
        fields[0].len() == 64 && fields[0].bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid SHA-256 checksum"
    );
    Ok(fields[0].to_ascii_lowercase())
}
fn file_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let len = file.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        hash.update(&buffer[..len]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn extract(archive: &Path, target: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    // The release workflow packages exactly ShiTu.exe. Never extract paths
    // supplied by ZIP entries, or accept extra files/scripts beside the binary.
    ensure!(zip.len() == 1, "Update archive must contain only ShiTu.exe");
    let mut entry = zip.by_index(0)?;
    ensure!(
        entry.name() == "ShiTu.exe" && entry.is_file(),
        "Unexpected update archive entry"
    );
    ensure!(
        entry.size() > 0 && entry.size() <= MAX_EXE,
        "Invalid executable size"
    );
    ensure!(
        entry
            .unix_mode()
            .is_none_or(|mode| mode & 0o170000 != 0o120000),
        "Symlinks are not allowed"
    );
    let expected_size = entry.size();
    let mut output = File::create_new(target)?;
    let written = std::io::copy(&mut Read::by_ref(&mut entry).take(MAX_EXE + 1), &mut output)?;
    ensure!(
        written == expected_size && written <= MAX_EXE,
        "Incomplete executable"
    );
    output.sync_all()?;
    Ok(())
}

pub struct Prepared {
    directory: PathBuf,
    keep: bool,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        // Only our known staging files; no recursive deletion of computed paths.
        for name in [
            "archive.zip",
            "new.exe",
            "updater.exe",
            "plan.json",
            "ready",
            "cancel",
        ] {
            let _ = fs::remove_file(self.directory.join(name));
        }
        let _ = fs::remove_dir(&self.directory);
    }
}
#[derive(Serialize, Deserialize)]
struct Plan {
    target: PathBuf,
    parent_pid: u32,
    old_hash: String,
    new_hash: String,
    version: String,
}
pub fn prepare(release: &Release, mut progress: impl FnMut(u8)) -> Result<Prepared> {
    ensure!(
        install::is_portable()?,
        "This installation must be updated through its original distribution channel"
    );
    ensure!(
        Version::parse(&release.version)? > Version::parse(env!("CARGO_PKG_VERSION"))?,
        "Downgrades are not allowed"
    );
    let target = std::env::current_exe()?.canonicalize()?;
    let directory = target
        .parent()
        .context("Missing installation directory")?
        .join(format!(
            ".shitu-update-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
    fs::create_dir(&directory).context("Cannot write to the installation directory")?;
    let prepared = Prepared {
        directory,
        keep: false,
    };
    let agent = agent();
    let response = agent
        .get(&release.checksum.browser_download_url)
        .timeout(Duration::from_secs(20))
        .call()?;
    let bytes = bounded_bytes(response.into_reader(), 4096)?;
    let expected = checksum(&bytes, &release.archive.name)?;
    let response = agent
        .get(&release.archive.browser_download_url)
        .timeout(Duration::from_secs(180))
        .call()?;
    let mut reader = response.into_reader();
    let archive = prepared.directory.join("archive.zip");
    let mut file = File::create_new(&archive)?;
    let mut total = 0u64;
    let mut last = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        ensure!(
            total <= release.archive.size && total <= MAX_ARCHIVE,
            "Download exceeds expected size"
        );
        file.write_all(&buffer[..count])?;
        let percent = (total * 100 / release.archive.size) as u8;
        if percent != last {
            progress(percent);
            last = percent;
        }
    }
    file.sync_all()?;
    drop(file);
    ensure!(total == release.archive.size, "Incomplete download");
    ensure!(
        file_hash(&archive)? == expected,
        "SHA-256 verification failed"
    );
    let candidate = prepared.directory.join("new.exe");
    extract(&archive, &candidate)?;
    install::validate_binary(&candidate)?;
    let plan = Plan {
        target: target.clone(),
        parent_pid: std::process::id(),
        old_hash: file_hash(&target)?,
        new_hash: file_hash(&candidate)?,
        version: release.version.clone(),
    };
    fs::copy(&target, prepared.directory.join("updater.exe"))?;
    fs::write(
        prepared.directory.join("plan.json"),
        serde_json::to_vec(&plan)?,
    )?;
    fs::remove_file(archive)?;
    Ok(prepared)
}

#[cfg(test)]
mod tests;
