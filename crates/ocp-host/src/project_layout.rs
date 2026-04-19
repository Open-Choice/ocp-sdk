use std::path::PathBuf;

pub fn default_cache_root() -> PathBuf {
    if let Some(local_data_dir) = dirs::data_local_dir() {
        return local_data_dir
            .join("OpenChoice")
            .join("cache");
    }

    PathBuf::from(".open-choice-cache")
}

pub fn cache_path_for_release(
    family: &str,
    tool: &str,
    version: &str,
    platform: &str,
    sha256: &str,
    executable_name: &str,
) -> PathBuf {
    default_cache_root()
        .join(family)
        .join(tool)
        .join(version)
        .join(platform)
        .join(sha256)
        .join(executable_name)
}

pub fn project_bin_path(project_root: &std::path::Path, executable_name: &str) -> PathBuf {
    project_root.join("bin").join(executable_name)
}
