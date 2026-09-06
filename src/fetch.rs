use crate::cache;
use crate::print;
use std::fs;
use std::io::copy;
use std::path::Path;

pub fn list_pkgs_from_url(repo_url: &str) -> Vec<String> {
    list_versions_from_url(repo_url)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

pub fn list_versions_from_url(repo_url: &str) -> Vec<(String, String)> {
    let url = format!("{}/pkgs.list", repo_url);
    if let Ok(mut resp) = ureq::get(&url).call() {
        if let Ok(text) = resp.body_mut().read_to_string() {
            return text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| {
                    let mut parts = l.split_whitespace();
                    let name = parts.next()?.to_string();
                    let version = parts
                        .next()
                        .unwrap_or("")
                        .trim_start_matches('v')
                        .to_string();
                    Some((name, version))
                })
                .collect();
        }
    }
    Vec::new()
}

pub fn fetch_pkg_to(url: &str, pkg_name: &str, dest: &str) {
    let repo = url.trim_end_matches('/');
    let urls = [
        format!("{}/vex/pkgs/{}.tar.zst", repo, pkg_name),
        format!("{}/{}.tar.zst", repo, pkg_name),
    ];
    for endpoint in &urls {
        if let Ok(response) = ureq::get(endpoint).call() {
            if response.status() == 200 {
                let mut file = match fs::File::create(dest) {
                    Ok(f) => f,
                    Err(e) => {
                        print::vex_error(&format!("failed to create tarball: {}", e));
                        return;
                    }
                };
                let body = response.into_body();
                let mut reader = body.into_with_config().limit(u64::MAX).reader();
                if let Err(e) = copy(&mut reader, &mut file) {
                    print::vex_error(&format!("failed to write tarball: {}", e));
                }
                return;
            }
        }
    }
    print::vex_error(&format!("failed to fetch '{}' from any URL", pkg_name));
    std::process::exit(1);
}

pub fn get_build_vex(pkg_name: &str) -> Option<String> {
    let path = format!("{}/build_vex/{}.buildvex", cache::cache_dir(), pkg_name);
    if Path::new(&path).exists() {
        return fs::read_to_string(&path).ok();
    }
    None
}

pub fn set_build_vex(pkg_name: &str, content: &str) {
    let path = format!("{}/build_vex/{}.buildvex", cache::cache_dir(), pkg_name);
    fs::create_dir_all(Path::new(&path).parent().unwrap()).ok();
    fs::write(&path, content).ok();
}
