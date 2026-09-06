use std::env;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

fn home() -> String {
    env::var("HOME").expect("Failed to get HOME")
}

pub fn cache_dir() -> String {
    format!("{}/.vex/cache", home())
}

fn repo_hash(repo_url: &str) -> String {
    // simple hash — just sanitize the url into a valid dirname
    repo_url
        .replace("://", "_")
        .replace("/", "_")
        .replace(".", "_")
}

fn pkg_version_path(repo_url: &str, pkg_name: &str) -> String {
    format!(
        "{}/{}/{}.version",
        cache_dir(),
        repo_hash(repo_url),
        pkg_name
    )
}

fn pkglist_path(repo_url: &str) -> String {
    format!("{}/{}/pkglist", cache_dir(), repo_hash(repo_url))
}

fn is_stale(path: &str, ttl_hours: u64) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = meta.modified() else {
        return true;
    };
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    age.as_secs() > ttl_hours * 3600
}

pub fn get_pkglist(repo_url: &str, ttl_hours: u64) -> Option<Vec<String>> {
    let path = pkglist_path(repo_url);
    if Path::new(&path).exists() && !is_stale(&path, ttl_hours) {
        let content = fs::read_to_string(&path).ok()?;
        return Some(content.lines().map(|l| l.to_string()).collect());
    }
    None
}

pub fn set_pkglist(repo_url: &str, pkgs: &[String]) {
    let path = pkglist_path(repo_url);
    fs::create_dir_all(Path::new(&path).parent().unwrap()).ok();
    fs::write(&path, pkgs.join("\n")).ok();
}

pub fn get_version(repo_url: &str, pkg_name: &str, ttl_hours: u64) -> Option<String> {
    let path = pkg_version_path(repo_url, pkg_name);
    if Path::new(&path).exists() && !is_stale(&path, ttl_hours) {
        return fs::read_to_string(&path).ok();
    }
    None
}

pub fn set_version(repo_url: &str, pkg_name: &str, version: &str) {
    let path = pkg_version_path(repo_url, pkg_name);
    fs::create_dir_all(Path::new(&path).parent().unwrap()).ok();
    fs::write(&path, version).ok();
}

pub fn invalidate_all() {
    fs::remove_dir_all(cache_dir()).ok();
}

pub fn get_versions_list(repo_url: &str, ttl_hours: u64) -> Option<Vec<(String, String)>> {
    let path = pkglist_path(repo_url);
    if Path::new(&path).exists() && !is_stale(&path, ttl_hours) {
        let content = fs::read_to_string(&path).ok()?;
        return Some(
            content
                .lines()
                .filter_map(|l| {
                    let mut parts = l.split_whitespace();
                    let name = parts.next()?.to_string();
                    let version = parts.next().unwrap_or("").to_string();
                    Some((name, version))
                })
                .collect(),
        );
    }
    None
}

pub fn set_versions_list(repo_url: &str, pkgs: &[String]) {
    // reuse set_pkglist — same file
    set_pkglist(repo_url, pkgs);
}
