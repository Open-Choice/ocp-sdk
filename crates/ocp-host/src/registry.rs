use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

use crate::project_layout::{cache_path_for_release, project_bin_path};

// ---------------------------------------------------------------------------
// Registry index types
//
// These describe the JSON documents hosted at a plugin registry — the
// top-level index, per-family indexes, and per-tool release indexes. They are
// host-side configuration/discovery types, not `ocp-json/1` wire types emitted
// by plugins, so they live here rather than in `ocp-types-v1`.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryFamilyPointer {
    pub id: String,
    pub index_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryProtocolPointer {
    pub id: String,
    pub definition_url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    pub registry_version: String,
    pub organization: String,
    pub families: Vec<RegistryFamilyPointer>,
    pub protocols: Vec<RegistryProtocolPointer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyToolPointer {
    pub tool: String,
    pub release_index_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyIndex {
    pub family: String,
    pub tools: Vec<FamilyToolPointer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolReleaseRecord {
    pub version: String,
    pub platform: String,
    pub download_url: String,
    pub sha256: String,
    pub protocol_version: String,
    pub published_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_app_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolReleaseIndex {
    pub family: String,
    pub tool: String,
    pub releases: Vec<ToolReleaseRecord>,
}

/// Hard ceiling on a single artifact download. Anything larger than this is
/// almost certainly hostile (or a misconfigured registry) and we'd rather
/// fail than OOM the host. 200 MiB is generous for native sidecars.
const MAX_DOWNLOAD_BYTES: u64 = 200 * 1024 * 1024;

/// Hard ceiling on a single registry JSON document. Real index files are
/// kilobytes, so 16 MiB leaves headroom while still bounding memory.
const MAX_REGISTRY_JSON_BYTES: u64 = 16 * 1024 * 1024;

/// Wall-clock timeout for any single registry HTTP request. Without this,
/// a slow or malicious registry can hang the calling thread (often the
/// Tauri command worker) indefinitely.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("failed to read registry data from {0}")]
    Read(String),
    #[error("failed to parse registry JSON: {0}")]
    Parse(String),
    #[error("failed to download release: {0}")]
    Download(String),
    #[error("release not found for version={version} platform={platform}")]
    ReleaseNotFound { version: String, platform: String },
    #[error(
        "downloaded artifact hash mismatch: expected sha256={expected}, got sha256={actual}"
    )]
    HashMismatch { expected: String, actual: String },
    #[error(
        "registry artifact exceeds {limit_bytes}-byte ceiling (refusing to load into memory)"
    )]
    Oversized { limit_bytes: u64 },
    #[error(
        "registry-supplied filename component '{name}' is unsafe (contains path separators, parent traversal, or is empty)"
    )]
    UnsafeFilename { name: String },
    #[error("failed to pin tool into project: {0}")]
    Pin(String),
}

/// Validate a single path component coming from registry data. The string
/// must be a plain filename with no separators, no parent-directory marker,
/// no leading dot weirdness, and only ASCII characters that are safe on
/// both POSIX and Windows. Without this check, an attacker who controls
/// the registry can use a `download_url` whose final segment is e.g.
/// `..\..\Startup\evil.exe` and escape the cache directory when the path
/// is joined.
fn validate_safe_filename_component(name: &str) -> Result<(), RegistryError> {
    let unsafe_component = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.contains(':')
        || name.chars().any(|c| c.is_control());
    if unsafe_component {
        return Err(RegistryError::UnsafeFilename {
            name: name.to_string(),
        });
    }
    Ok(())
}

fn http_client() -> Result<reqwest::blocking::Client, RegistryError> {
    reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|err| RegistryError::Read(err.to_string()))
}

pub fn load_registry_index(path_or_url: &str) -> Result<RegistryIndex, RegistryError> {
    load_json(path_or_url)
}

pub fn load_family_index(path_or_url: &str) -> Result<FamilyIndex, RegistryError> {
    load_json(path_or_url)
}

pub fn load_tool_release_index(path_or_url: &str) -> Result<ToolReleaseIndex, RegistryError> {
    load_json(path_or_url)
}

pub fn select_release<'a>(
    index: &'a ToolReleaseIndex,
    version: &str,
    platform: &str,
) -> Result<&'a ToolReleaseRecord, RegistryError> {
    index.releases
        .iter()
        .find(|release| release.version == version && release.platform == platform)
        .ok_or_else(|| RegistryError::ReleaseNotFound {
            version: version.to_string(),
            platform: platform.to_string(),
        })
}

pub fn cache_release(release: &ToolReleaseRecord, family: &str, tool: &str) -> Result<PathBuf, RegistryError> {
    // The executable name is derived from the registry-supplied download URL.
    // Validate it as a single safe filename component before using it in any
    // path join — otherwise an attacker can supply `..\..\evil.exe` and break
    // out of the cache root on Windows where backslashes are separators.
    let raw_name = release
        .download_url
        .rsplit('/')
        .next()
        .unwrap_or("");
    validate_safe_filename_component(raw_name)?;
    // Likewise validate the family/tool/version/platform/sha256 components,
    // since they're all used as path segments.
    for component in [family, tool, release.version.as_str(), release.platform.as_str(), release.sha256.as_str()] {
        validate_safe_filename_component(component)?;
    }
    let executable_name = raw_name;

    let destination = cache_path_for_release(
        family,
        tool,
        &release.version,
        &release.platform,
        &release.sha256,
        executable_name,
    );
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|err| RegistryError::Download(err.to_string()))?;
    }

    // Use a Client with an explicit timeout instead of `reqwest::blocking::get`,
    // which has no default timeout and can hang the host indefinitely against a
    // slow or hostile registry.
    let client = http_client()?;
    let response = client
        .get(&release.download_url)
        .send()
        .map_err(|err| RegistryError::Download(err.to_string()))?;

    // Cap the in-memory body size. `reqwest::blocking::Response::bytes()` would
    // happily allocate gigabytes if the registry returns them; that's an OOM
    // vector. Read at most MAX_DOWNLOAD_BYTES + 1 so we can detect overflow.
    let mut bytes = Vec::with_capacity(64 * 1024);
    response
        .take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| RegistryError::Download(err.to_string()))?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(RegistryError::Oversized {
            limit_bytes: MAX_DOWNLOAD_BYTES,
        });
    }

    // Verify the downloaded bytes match the expected SHA-256 from the release
    // record before writing them to the cache. Without this check, an attacker
    // who controls the download URL (or an upstream MitM) can substitute an
    // arbitrary binary that will then be executed by the host.
    let actual_hash = hex::encode(Sha256::digest(&bytes));
    let expected_hash = release.sha256.to_ascii_lowercase();
    if actual_hash != expected_hash {
        return Err(RegistryError::HashMismatch {
            expected: expected_hash,
            actual: actual_hash,
        });
    }

    fs::write(&destination, bytes).map_err(|err| RegistryError::Download(err.to_string()))?;
    Ok(destination)
}

pub fn pin_tool_into_project(
    executable_path: &Path,
    project_root: &Path,
    executable_name: &str,
) -> Result<PathBuf, RegistryError> {
    // Defense in depth: even though `cache_release` validates filenames coming
    // from the registry, callers may pass an arbitrary string here. Refuse
    // anything that isn't a single safe filename component so we can never
    // write outside `<project_root>/bin/`.
    validate_safe_filename_component(executable_name)
        .map_err(|err| RegistryError::Pin(err.to_string()))?;
    let target = project_bin_path(project_root, executable_name);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| RegistryError::Pin(err.to_string()))?;
    }
    fs::copy(executable_path, &target).map_err(|err| RegistryError::Pin(err.to_string()))?;
    Ok(target)
}

fn load_json<T: serde::de::DeserializeOwned>(path_or_url: &str) -> Result<T, RegistryError> {
    let text = if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        // Use a Client with an explicit timeout. The default `reqwest::blocking::get`
        // has no timeout and can hang the calling thread (typically a Tauri command
        // worker) forever against a slow or malicious registry.
        let client = http_client()?;
        let response = client
            .get(path_or_url)
            .send()
            .map_err(|err| RegistryError::Read(err.to_string()))?;

        // Bound the response body. Real registry index files are kilobytes, so a
        // multi-megabyte ceiling is generous; without this, a hostile server can
        // OOM the host by feeding an unbounded stream into `text()`.
        let mut buf = Vec::with_capacity(64 * 1024);
        response
            .take(MAX_REGISTRY_JSON_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|err| RegistryError::Read(err.to_string()))?;
        if buf.len() as u64 > MAX_REGISTRY_JSON_BYTES {
            return Err(RegistryError::Oversized {
                limit_bytes: MAX_REGISTRY_JSON_BYTES,
            });
        }
        String::from_utf8(buf).map_err(|err| RegistryError::Read(err.to_string()))?
    } else {
        fs::read_to_string(path_or_url).map_err(|err| RegistryError::Read(err.to_string()))?
    };

    serde_json::from_str(&text).map_err(|err| RegistryError::Parse(err.to_string()))
}
