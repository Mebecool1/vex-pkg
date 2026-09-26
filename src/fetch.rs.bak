use crate::cache;
use crate::print;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
pub fn list_pkgs_from_url(repo_url: &str) -> Vec<String> {
    list_versions_from_url(repo_url)
        .into_iter()
        .map(|(name, _, _)| name)
        .collect()
}
pub fn list_versions_from_url(repo_url: &str) -> Vec<(String, String, Vec<String>)> {
    let url = format!("{}/pkgs.list", repo_url);

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .timeout_resolve(Some(Duration::from_secs(10)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(10)))
        .timeout_recv_body(Some(Duration::from_secs(10)))
        .build()
        .into();
    if let Ok(mut resp) = agent.get(&url).call() {
        if let Ok(text) = resp.body_mut().read_to_string() {
            let result = text
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
                    let deps = parts
                        .find(|p| p.starts_with("deps="))
                        .map(|p| {
                            p.trim_start_matches("deps=")
                                .split(',')
                                .map(|s| s.to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    Some((name, version, deps))
                })
                .collect();
            return result;
        }
    }

    Vec::new()
}
pub fn fetch_pkg_to(url: &str, pkg_name: &str, dest: &str, multi: &indicatif::MultiProgress) {
    let repo = url.trim_end_matches('/');
    let urls = [
        format!("{}/vex/pkgs/{}.tar.zst", repo, pkg_name),
        format!("{}/{}.tar.zst", repo, pkg_name),
    ];
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into();
    for endpoint in &urls {
        if let Ok(response) = agent.get(endpoint).call() {
            if response.status() == 200 {
                let total = response
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);

                let raw_pb = if total > 0 {
                    let pb = indicatif::ProgressBar::new(total);
                    pb.set_style(
                        indicatif::ProgressStyle::with_template(
                            "{prefix:.cyan.bold} [{bar:30}] {bytes}/{total_bytes} {bytes_per_sec} {eta}",
                        )
                        .unwrap()
                        .progress_chars("=>-"),
                    );
                    pb
                } else {
                    let pb = indicatif::ProgressBar::new_spinner();
                    pb.set_style(
                        indicatif::ProgressStyle::with_template(
                            "{prefix:.cyan.bold} {spinner} {bytes} {bytes_per_sec}",
                        )
                        .unwrap(),
                    );
                    pb
                };
                let pb = multi.add(raw_pb);
                pb.set_prefix(format!("Fetching {}", pkg_name));
                pb.enable_steady_tick(std::time::Duration::from_millis(80));

                let mut file = match fs::File::create(dest) {
                    Ok(f) => f,
                    Err(e) => {
                        print::vex_error(&format!("failed to create tarball: {}", e));
                        return;
                    }
                };
                let body = response.into_body();
                let mut reader = body.into_with_config().limit(u64::MAX).reader();
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            file.write_all(&buf[..n]).ok();
                            pb.inc(n as u64);
                        }
                        Err(e) => {
                            print::vex_error(&format!("failed to write tarball: {}", e));
                            break;
                        }
                    }
                }
                pb.finish_with_message("done");
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
