use crate::print;
use std::fs;
use std::io::copy;
pub fn list_pkgs_from_url(repo: &str) -> Vec<String> {
    let repo = repo.trim_end_matches('/');

    let urls = [
        format!("{}/vex/pkgs/pkgs.list", repo),
        format!("{}/pkgs.list", repo),
    ];

    for url in &urls {
        match ureq::get(url).call() {
            Ok(mut response) => {
                if response.status() == 200 {
                    let body = response.body_mut().read_to_string().unwrap_or_default();
                    let pkgs: Vec<String> = body
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();
                    if !pkgs.is_empty() {
                        return pkgs;
                    }
                }
            }
            Err(_) => continue,
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

    panic!("Failed to fetch package '{}' from any URL", pkg_name);
}
