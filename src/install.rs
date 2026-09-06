use crate::cache;
use crate::fetch;
use crate::lockfile;
use crate::print;
use crate::vex_lang;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;

// ── helpers ───────────────────────────────────────────────────────────────────

fn home() -> String {
    env::var("HOME").expect("Failed to get HOME")
}

fn vex_pkgs_dir() -> String {
    format!("{}/.vex/pkgs", home())
}

fn pkg_dir(pkg_name: &str) -> String {
    format!("{}/{}", vex_pkgs_dir(), pkg_name)
}

fn tar_path(pkg_name: &str) -> String {
    format!("{}/{}.tar.zst", vex_pkgs_dir(), pkg_name)
}

fn manifest_path(pkg_name: &str) -> String {
    format!("{}/.vex_manifest", pkg_dir(pkg_name))
}

pub fn is_installed(pkg_name: &str) -> bool {
    Path::new(&pkg_dir(pkg_name)).exists() && Path::new(&manifest_path(pkg_name)).exists()
}

fn installed_pkgs() -> HashSet<String> {
    let dir = vex_pkgs_dir();
    match fs::read_dir(&dir) {
        Err(_) => HashSet::new(),
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
    }
}

// ── manifest ──────────────────────────────────────────────────────────────────

enum ManifestEntry {
    File(PathBuf),
    Dir(PathBuf),
}

struct Manifest {
    version: String,
    entries: Vec<ManifestEntry>,
}

impl Manifest {
    fn new(version: String) -> Self {
        Self {
            version,
            entries: Vec::new(),
        }
    }

    fn record_file(&mut self, path: impl Into<PathBuf>) {
        self.entries.push(ManifestEntry::File(path.into()));
    }

    fn record_dir(&mut self, path: impl Into<PathBuf>) {
        self.entries.push(ManifestEntry::Dir(path.into()));
    }

    fn save(&self, path: &str) -> Result<(), String> {
        let mut lines = vec![format!("V {}", self.version)];
        lines.extend(self.entries.iter().map(|e| match e {
            ManifestEntry::File(p) => format!("F {}", p.display()),
            ManifestEntry::Dir(p) => format!("D {}", p.display()),
        }));
        fs::write(path, lines.join("\n")).map_err(|e| format!("Failed to write manifest: {}", e))
    }

    fn load(path: &str) -> Result<Self, String> {
        let content =
            fs::read_to_string(path).map_err(|_| format!("No manifest found at {}", path))?;
        let mut lines = content.lines().filter(|l| l.len() > 2);
        let version = lines
            .next()
            .filter(|l| l.starts_with('V'))
            .map(|l| l[2..].to_string())
            .unwrap_or_default();
        let entries = lines
            .map(|l| {
                let p = PathBuf::from(&l[2..]);
                if l.starts_with('D') {
                    ManifestEntry::Dir(p)
                } else {
                    ManifestEntry::File(p)
                }
            })
            .collect();
        Ok(Self { entries, version })
    }

    fn uninstall(&self, pkg_name: &str) {
        for entry in self.entries.iter().rev() {
            match entry {
                ManifestEntry::File(p) => {
                    if p.exists() {
                        match fs::remove_file(p) {
                            Ok(_) => {}
                            Err(e) => {
                                print::vex_warn(&format!("could not remove {}: {}", p.display(), e))
                            }
                        }
                    }
                }
                ManifestEntry::Dir(p) => {
                    if let Ok(mut rd) = fs::read_dir(p) {
                        if rd.next().is_none() {
                            match fs::remove_dir(p) {
                                Ok(_) => print::vex_print("Removed", &format!("{}", p.display())),
                                Err(e) => print::vex_warn(&format!(
                                    "could not remove dir {}: {}",
                                    p.display(),
                                    e
                                )),
                            }
                        } else {
                            let _ = fs::remove_dir_all(p);
                        }
                    }
                }
            }
        }
        if let Err(e) = fs::remove_dir_all(pkg_dir(pkg_name)) {
            print::vex_warn(&format!("could not remove pkg dir: {}", e));
        } else {
            print::vex_print("Removed", &format!("pkg dir for {}", pkg_name));
        }
    }
}

// ── filesystem snapshot ───────────────────────────────────────────────────────

fn watch_roots() -> Vec<PathBuf> {
    let h = home();
    vec![
        PathBuf::from(format!("{}/.vex", h)),
        PathBuf::from(format!("{}/.local", h)),
        PathBuf::from(format!("{}/.config", h)),
        PathBuf::from(format!("{}/.bin", h)),
        PathBuf::from(format!("{}/bin", h)),
        PathBuf::from(format!("{}/.vex/lib", h)),
        PathBuf::from(format!("{}/.vex/bin", h)),
    ]
}

fn snapshot_paths(roots: &[PathBuf]) -> HashSet<PathBuf> {
    let mut paths = HashSet::new();
    for root in roots {
        collect_paths(root, &mut paths);
    }
    paths
}

fn collect_paths(dir: &Path, out: &mut HashSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        out.insert(path.clone());
        if path.is_dir() {
            collect_paths(&path, out);
        }
    }
}

fn diff_snapshots(
    before: &HashSet<PathBuf>,
    after: &HashSet<PathBuf>,
    exclude_prefix: Option<&Path>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut new_paths: Vec<&PathBuf> = after
        .difference(before)
        .filter(|p| {
            exclude_prefix
                .map(|prefix| !p.starts_with(prefix))
                .unwrap_or(true)
        })
        .collect();
    new_paths.sort_by_key(|p| p.components().count());
    let mut new_dirs = Vec::new();
    let mut new_files = Vec::new();
    for p in new_paths {
        if p.is_dir() {
            new_dirs.push(p.clone());
        } else {
            new_files.push(p.clone());
        }
    }
    (new_dirs, new_files)
}

// ── dependency resolution ─────────────────────────────────────────────────────

fn resolve_deps(
    pkg_name: &str,
    repos: &[String],
    visited: &mut HashSet<String>,
    needed: &mut Vec<String>,
) -> Result<(), String> {
    if visited.contains(pkg_name) {
        return Ok(());
    }
    visited.insert(pkg_name.to_string());
    let build_content = fetch_build_vex(pkg_name, repos)?;
    let parsed = vex_lang::parse_vex(&build_content);
    for dep in vex_lang::get_values(&parsed, "dependencies") {
        if !dep.is_empty() {
            resolve_deps(&dep, repos, visited, needed)?;
        }
    }
    needed.push(pkg_name.to_string());
    Ok(())
}

// ── fetch build.vex ───────────────────────────────────────────────────────────

fn fetch_build_vex(pkg_name: &str, repos: &[String]) -> Result<String, String> {
    let installed_build = format!("{}/build.vex", pkg_dir(pkg_name));
    if let Ok(content) = fs::read_to_string(&installed_build) {
        return Ok(content);
    }
    let tar = tar_path(pkg_name);
    fs::create_dir_all(vex_pkgs_dir()).map_err(|e| e.to_string())?;
    let mut fetched = false;
    for repo in repos {
        if fetch::list_pkgs_from_url(repo)
            .iter()
            .any(|p| p.trim_end_matches(".tar.zst").trim_end_matches(".tar") == pkg_name)
        {
            fetch::fetch_pkg_to(repo, pkg_name, &tar);
            fetched = true;
            break;
        }
    }
    if !fetched {
        return Err(format!("package '{}' not found in any repo", pkg_name));
    }
    let tmp_dir = format!("{}/.vex/.tmp_{}", home(), pkg_name);
    fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let extract_status = std::process::Command::new("tar")
        .args(["-xf", &tar, "-C", &tmp_dir])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| {
            let _ = fs::remove_dir_all(&tmp_dir);
            format!("Failed to run tar: {}", e)
        })?;
    if !extract_status.success() {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(format!("tar failed for package '{}'", pkg_name));
    }
    fs::remove_file(&tar).ok();
    let content = fs::read_to_string(format!("{}/build.vex", tmp_dir)).map_err(|_| {
        let _ = fs::remove_dir_all(&tmp_dir);
        format!("build.vex missing in package '{}'", pkg_name)
    })?;
    let real_dir = pkg_dir(pkg_name);
    if Path::new(&real_dir).exists() {
        fs::remove_dir_all(&real_dir).map_err(|e| {
            let _ = fs::remove_dir_all(&tmp_dir);
            e.to_string()
        })?;
    }
    fs::rename(&tmp_dir, &real_dir).map_err(|e| {
        let _ = fs::remove_dir_all(&tmp_dir);
        format!("Failed to promote tmp dir: {}", e)
    })?;
    Ok(content)
}

// ── info ──────────────────────────────────────────────────────────────────────

pub fn info(pkg_name: &str, repos: &[String]) {
    let content = match fetch_build_vex(pkg_name, repos) {
        Ok(c) => c,
        Err(e) => {
            print::vex_error(&format!(
                "could not fetch build.vex for '{}': {}",
                pkg_name, e
            ));
            return;
        }
    };
    let parsed = vex_lang::parse_vex(&content);
    let name = vex_lang::get_values(&parsed, "name")
        .into_iter()
        .next()
        .unwrap_or_default();
    let version = vex_lang::get_values(&parsed, "version")
        .into_iter()
        .next()
        .unwrap_or_default();
    let deps = vex_lang::get_values(&parsed, "dependencies");
    let cmds = vex_lang::get_values(&parsed, "commands");
    print::vex_print("Package", pkg_name);
    if !name.is_empty() {
        print::vex_print("Name", &name);
    }
    if !version.is_empty() {
        print::vex_print("Version", &version);
    }
    if deps.is_empty() || deps.iter().all(|d| d.is_empty()) {
        print::vex_print("Deps", "none");
    } else {
        print::vex_print("Deps", &deps.join("  "));
    }
    print::vex_print("Commands", "");
    for cmd in cmds {
        print::vex_print("", &format!("  $ {}", cmd));
    }
    print::vex_print(
        "Installed",
        if is_installed(pkg_name) { "yes" } else { "no" },
    );
}

// ── install_pkg_no_spinner (for parallel builds) ──────────────────────────────

fn install_pkg_no_spinner(pkg_name: &str, repos: &[String], force: bool) -> Result<(), String> {
    let dir = pkg_dir(pkg_name);
    if Path::new(&dir).exists()
        && !Path::new(&manifest_path(pkg_name)).exists()
        && Path::new(&format!("{}/build.vex", dir)).exists()
        && !fs::read_dir(&dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
    {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    } else if is_installed(pkg_name) && !force {
        return Ok(());
    } else if force {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    if !Path::new(&dir).exists() {
        let tar = tar_path(pkg_name);
        fs::create_dir_all(vex_pkgs_dir()).map_err(|e| e.to_string())?;
        if !Path::new(&tar).exists() {
            let mut fetched = false;
            for repo in repos {
                if fetch::list_pkgs_from_url(repo)
                    .iter()
                    .any(|p| p.trim_end_matches(".tar.zst").trim_end_matches(".tar") == pkg_name)
                {
                    fetch::fetch_pkg_to(repo, pkg_name, &tar);
                    fetched = true;
                    break;
                }
            }
            if !fetched {
                return Err(format!("'{}' not found in any repo", pkg_name));
            }
        }
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::process::Command::new("tar")
            .args(["-xf", &tar, "-C", &dir])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap_or_else(|e| {
                print::vex_error(&format!("failed to extract tarball: {}", e));
                std::process::exit(1);
            });
        fs::remove_file(&tar).ok();
    }
    let build_content = fs::read_to_string(format!("{}/build.vex", dir))
        .map_err(|_| format!("build.vex missing for '{}'", pkg_name))?;
    let parsed = vex_lang::parse_vex(&build_content);
    let commands = vex_lang::get_values(&parsed, "commands");
    let version = vex_lang::get_values(&parsed, "version")
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            print::vex_error("build.vex is missing a 'version' field");
            std::process::exit(1);
        });
    let install_to = vex_lang::get_values(&parsed, "install-to");
    for cmd in &commands {
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("Failed to run '{}': {}", cmd, e))?;
        if !status.success() {
            return Err(format!(
                "command '{}' failed with exit code {:?}",
                cmd,
                status.code()
            ));
        }
    }
    let mut manifest = Manifest::new((*version).to_string());
    for entry in &install_to {
        let full_path = format!("{}/.vex/{}", home(), entry);
        if Path::new(&full_path).is_dir() {
            manifest.record_dir(&full_path);
        } else {
            manifest.record_file(&full_path);
        }
    }
    manifest.save(&manifest_path(pkg_name))?;
    lockfile::update(pkg_name, &version);
    Ok(())
}

// ── install_pkg (sequential, with spinner) ────────────────────────────────────

pub fn install_pkg(pkg_name: &str, repos: &[String], force: bool) -> Result<(), String> {
    let dir = pkg_dir(pkg_name);
    if Path::new(&dir).exists()
        && !Path::new(&manifest_path(pkg_name)).exists()
        && Path::new(&format!("{}/build.vex", dir)).exists()
        && !fs::read_dir(&dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
    {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    } else if is_installed(pkg_name) && !force {
        print::vex_print("Fresh", pkg_name);
        return Ok(());
    } else if force {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        print::vex_print("Updating", pkg_name);
    } else {
        print::vex_print("Installing", pkg_name);
    }
    if !Path::new(&dir).exists() {
        let tar = tar_path(pkg_name);
        fs::create_dir_all(vex_pkgs_dir()).map_err(|e| e.to_string())?;
        if !Path::new(&tar).exists() {
            let mut fetched = false;
            for repo in repos {
                if fetch::list_pkgs_from_url(repo)
                    .iter()
                    .any(|p| p.trim_end_matches(".tar.zst").trim_end_matches(".tar") == pkg_name)
                {
                    fetch::fetch_pkg_to(repo, pkg_name, &tar);
                    fetched = true;
                    break;
                }
            }
            if !fetched {
                return Err(format!("'{}' not found in any repo", pkg_name));
            }
        }
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::process::Command::new("tar")
            .args(["-xf", &tar, "-C", &dir])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap_or_else(|e| {
                print::vex_error(&format!("failed to extract tarball: {}", e));
                std::process::exit(1);
            });
        fs::remove_file(&tar).ok();
    }
    let build_content = fs::read_to_string(format!("{}/build.vex", dir))
        .map_err(|_| format!("build.vex missing for '{}'", pkg_name))?;
    let parsed = vex_lang::parse_vex(&build_content);
    let commands = vex_lang::get_values(&parsed, "commands");
    let version = vex_lang::get_values(&parsed, "version")
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            print::vex_error("build.vex is missing a 'version' field");
            std::process::exit(1);
        });
    let install_to = vex_lang::get_values(&parsed, "install-to");
    let roots = watch_roots();
    let before = if install_to.is_empty() {
        Some(snapshot_paths(&roots))
    } else {
        None
    };
    let done = Arc::new(AtomicBool::new(false));
    let done_clone = done.clone();
    let pkg = pkg_name.to_string();
    println!();
    let spinner_thread = std::thread::spawn(move || {
        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let mut i = 0;
        while !done_clone.load(Ordering::Relaxed) {
            print!(
                "\r{:>12} {} {}...",
                frames[i % frames.len()].cyan().bold(),
                "Compiling".cyan().bold(),
                pkg
            );
            std::io::stdout().flush().unwrap();
            i += 1;
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
    });
    for cmd in &commands {
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| {
                done.store(true, Ordering::Relaxed);
                format!("Failed to run '{}': {}", cmd, e)
            })?;
        if !status.success() {
            done.store(true, Ordering::Relaxed);
            let _ = spinner_thread.join();
            return Err(format!(
                "command '{}' failed with exit code {:?}",
                cmd,
                status.code()
            ));
        }
    }
    done.store(true, Ordering::Relaxed);
    let _ = spinner_thread.join();
    print!("\r{:>13}\n", " ");
    let mut manifest = Manifest::new((*version).to_string());
    if install_to.is_empty() {
        let after = snapshot_paths(&roots);
        let (new_dirs, new_files) = diff_snapshots(&before.unwrap(), &after, Some(Path::new(&dir)));
        for d in &new_dirs {
            manifest.record_dir(d);
        }
        for f in &new_files {
            manifest.record_file(f);
        }
    } else {
        for entry in &install_to {
            let full_path = format!("{}/.vex/{}", home(), entry);
            if Path::new(&full_path).is_dir() {
                manifest.record_dir(&full_path);
            } else {
                manifest.record_file(&full_path);
            }
        }
    }
    manifest.save(&manifest_path(pkg_name))?;
    lockfile::update(pkg_name, &version);
    Ok(())
}

// ── remove ────────────────────────────────────────────────────────────────────

fn remove_pkg(pkg_name: &str, _repos: &[String]) {
    print::vex_print("Removing", pkg_name);
    match Manifest::load(&manifest_path(pkg_name)) {
        Ok(manifest) => {
            manifest.uninstall(pkg_name);
        }
        Err(_) => {
            print::vex_warn(&format!(
                "no manifest for '{}' — removing pkg dir only.",
                pkg_name
            ));
            print::vex_warn(&format!(
                "files installed outside ~/.vex/pkgs/{} were NOT cleaned up.",
                pkg_name
            ));
            let _ = fs::remove_dir_all(pkg_dir(pkg_name));
        }
    }
    lockfile::remove(pkg_name);
    print::vex_print("Removed", pkg_name);
}

// ── state machine: sync ───────────────────────────────────────────────────────

pub fn sync(desired_pkgs: &[String], repos: &[String], ttl: u64, locked: bool) {
    let start = std::time::Instant::now();

    fn peek_version(pkg_name: &str, repos: &[String], ttl_hours: u64) -> String {
        for repo in repos {
            if let Some(v) = cache::get_version(repo, pkg_name, ttl_hours) {
                return v;
            }
        }
        let tar = tar_path(pkg_name);
        fs::remove_file(&tar).ok();
        for repo in repos {
            if fetch::list_pkgs_from_url(repo)
                .iter()
                .any(|p| p.trim_end_matches(".tar.zst").trim_end_matches(".tar") == pkg_name)
            {
                fetch::fetch_pkg_to(repo, pkg_name, &tar);
                let tmp = format!("{}/.vex/.tmp_peek_{}", home(), pkg_name);
                fs::create_dir_all(&tmp).ok();
                std::process::Command::new("tar")
                    .args(["-xf", &tar, "-C", &tmp])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .ok();
                let content = fs::read_to_string(format!("{}/build.vex", tmp)).unwrap_or_default();
                fs::remove_dir_all(&tmp).ok();
                fs::remove_file(&tar).ok();
                let version = vex_lang::get_values(&vex_lang::parse_vex(&content), "version")
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                cache::set_version(repo, pkg_name, &version);
                return version;
            }
        }
        String::new()
    }

    fn confirm(prompt: &str) -> bool {
        print!("{} {} ", "::".cyan().bold(), prompt.bold());
        print!("{}", "[Y/n] ".dimmed());
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        matches!(input.trim().to_lowercase().as_str(), "y" | "yes" | "")
    }

    fn needs_update(pkg_name: &str, repos: &[String], ttl_hours: u64, locked: bool) -> bool {
        let local = Manifest::load(&manifest_path(pkg_name))
            .map(|m| m.version)
            .unwrap_or_default();
        if locked {
            let lock = lockfile::read();
            let pinned = lock.get(pkg_name).cloned().unwrap_or_default();
            return local != pinned;
        }
        let remote = peek_version(pkg_name, repos, ttl_hours);
        local != remote
    }

    let mut visited = HashSet::new();
    let mut needed: Vec<String> = Vec::new();
    for pkg in desired_pkgs {
        if let Err(e) = resolve_deps(pkg, repos, &mut visited, &mut needed) {
            print::vex_error(&e);
            print::vex_error("aborting sync — system state unchanged.");
            return;
        }
    }

    let currently_installed = installed_pkgs();
    let to_install: Vec<_> = needed.iter().filter(|p| !is_installed(p)).collect();
    // before building to_update, parallel-peek all installed pkgs
    let needed_arc = Arc::new(needed.clone());
    let repos_arc = Arc::new(repos.to_vec());
    let remote_versions: std::collections::HashMap<String, String> = {
        let handles: Vec<_> = needed_arc
            .iter()
            .filter(|p| is_installed(p))
            .map(|pkg| {
                let pkg = pkg.clone();
                let repos = repos_arc.clone();

                thread::spawn(move || {
                    let v = peek_version(&pkg, &repos, ttl);
                    (pkg, v)
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    };

    let to_update: Vec<_> = needed
        .iter()
        .filter(|p| {
            is_installed(p) && {
                let local = Manifest::load(&manifest_path(p))
                    .map(|m| m.version)
                    .unwrap_or_default();
                let remote = remote_versions.get(*p).cloned().unwrap_or_default();
                local != remote
            }
        })
        .collect();
    if !to_install.is_empty() {
        print::vex_print(
            "To Install",
            &to_install
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join("  "),
        );
    }
    if !to_update.is_empty() {
        print::vex_print(
            "To Update",
            &to_update
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join("  "),
        );
    }

    if !to_install.is_empty() || !to_update.is_empty() {
        if !confirm("Proceed with installation/update?") {
            print::vex_error("Installation/Update declined.");
            std::process::exit(1);
        }

        // ── parallel fetch ────────────────────────────────────────────────
        let all_to_get: Vec<String> = to_install
            .iter()
            .chain(to_update.iter())
            .map(|p| p.to_string())
            .collect();

        let multi_fetch = Arc::new(MultiProgress::new());
        let fetch_style = ProgressStyle::default_spinner()
            .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣿"])
            .template("    {spinner:.cyan.bold} {msg}")
            .unwrap();
        let repos_fetch = Arc::new(repos.to_vec());

        let fetch_handles: Vec<_> = all_to_get
            .into_iter()
            .map(|pkg| {
                let repos = repos_fetch.clone();
                let multi = multi_fetch.clone();
                let style = fetch_style.clone();
                thread::spawn(move || {
                    let tar = tar_path(&pkg);
                    if Path::new(&tar).exists() {
                        return;
                    }
                    for repo in repos.iter() {
                        if fetch::list_pkgs_from_url(repo).iter().any(|p| {
                            p.trim_end_matches(".tar.zst").trim_end_matches(".tar") == pkg.as_str()
                        }) {
                            let pb = multi.add(ProgressBar::new_spinner());
                            pb.set_style(style.clone());
                            pb.set_message(format!("{} {}...", "Fetching".cyan().bold(), pkg));
                            pb.enable_steady_tick(std::time::Duration::from_millis(80));
                            fetch::fetch_pkg_to(repo, &pkg, &tar);
                            pb.finish_with_message(format!("{} {}", "Fetched".green().bold(), pkg));
                            return;
                        }
                    }
                })
            })
            .collect();

        for handle in fetch_handles {
            handle.join().unwrap();
        }

        // ── split parallel (install-to) vs sequential (snapshot) ─────────
        let all_to_build: Vec<(String, bool)> = to_install
            .iter()
            .map(|p| (p.to_string(), false))
            .chain(to_update.iter().map(|p| (p.to_string(), true)))
            .collect();

        let mut parallel: Vec<(String, bool)> = Vec::new();
        let mut sequential: Vec<(String, bool)> = Vec::new();

        for (pkg, force) in &all_to_build {
            let build_path = format!("{}/build.vex", pkg_dir(pkg));
            let has_install_to = fs::read_to_string(&build_path)
                .map(|content| {
                    let parsed = vex_lang::parse_vex(&content);
                    !vex_lang::get_values(&parsed, "install-to").is_empty()
                })
                .unwrap_or(false);
            if has_install_to {
                parallel.push((pkg.clone(), *force));
            } else {
                sequential.push((pkg.clone(), *force));
            }
        }

        // ── parallel builds ───────────────────────────────────────────────
        println!();
        print::vex_print("Compiling", "packages");
        if !parallel.is_empty() {
            let multi = Arc::new(MultiProgress::new());
            let spinner_style = ProgressStyle::default_spinner()
                .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣿"])
                .template("    {spinner:.cyan.bold} {msg}")
                .unwrap();
            let repos_arc = Arc::new(repos.to_vec());

            let handles: Vec<_> = parallel
                .into_iter()
                .map(|(pkg, force)| {
                    let repos = repos_arc.clone();
                    let multi = multi.clone();
                    let style = spinner_style.clone();
                    thread::spawn(move || {
                        let pb = multi.add(ProgressBar::new_spinner());
                        pb.set_style(style);
                        pb.set_message(format!("{} {}...", "Compiling".cyan().bold(), pkg));
                        pb.enable_steady_tick(std::time::Duration::from_millis(80));
                        let result = install_pkg_no_spinner(&pkg, &repos, force);
                        match &result {
                            Ok(_) => pb.finish_with_message(format!(
                                "{} {}",
                                "Compiled".green().bold(),
                                pkg
                            )),
                            Err(e) => pb.finish_with_message(format!(
                                "{} {}: {}",
                                "Failed".red().bold(),
                                pkg,
                                e
                            )),
                        }
                        result
                    })
                })
                .collect();

            for handle in handles {
                if let Err(e) = handle.join().unwrap() {
                    print::vex_error(&format!("error: {}", e));
                    print::vex_error("aborting sync — some packages may be partially installed.");
                    return;
                }
            }
        }

        // ── sequential builds ─────────────────────────────────────────────
        for (pkg, force) in &sequential {
            if let Err(e) = install_pkg(pkg, repos, *force) {
                print::vex_error(&format!("error installing '{}': {}", pkg, e));
                print::vex_error("aborting sync — some packages may be partially installed.");
                return;
            }
        }
    }

    let to_remove: Vec<_> = currently_installed
        .iter()
        .filter(|p| !needed.contains(*p))
        .collect();
    if !to_remove.is_empty() {
        for pkg in &to_remove {
            remove_pkg(pkg, repos);
        }
    }

    if !Path::new(&lockfile::lock_path()).exists() {
        for pkg in &needed {
            if let Ok(manifest) = Manifest::load(&manifest_path(pkg)) {
                lockfile::update(pkg, &manifest.version);
            }
        }
    }

    print::vex_print(
        "Finished",
        &format!("sync in {:.2}s", start.elapsed().as_secs_f64()),
    );
}

// ── upgrade ───────────────────────────────────────────────────────────────────

pub fn upgrade(pkg_name: &str, repos: &[String], ttl: u64) {
    if !is_installed(pkg_name) {
        print::vex_error(&format!("'{}' is not installed", pkg_name));
        std::process::exit(1);
    }

    // peek remote version
    let tar = tar_path(pkg_name);
    fs::remove_file(&tar).ok();
    let mut remote_version = String::new();
    for repo in repos {
        if fetch::list_pkgs_from_url(repo)
            .iter()
            .any(|p| p.trim_end_matches(".tar.zst").trim_end_matches(".tar") == pkg_name)
        {
            if let Some(v) = cache::get_version(repo, pkg_name, ttl) {
                remote_version = v;
            } else {
                fetch::fetch_pkg_to(repo, pkg_name, &tar);
                let tmp = format!("{}/.vex/.tmp_peek_{}", home(), pkg_name);
                fs::create_dir_all(&tmp).ok();
                std::process::Command::new("tar")
                    .args(["-xf", &tar, "-C", &tmp])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .ok();
                let content = fs::read_to_string(format!("{}/build.vex", tmp)).unwrap_or_default();
                fs::remove_dir_all(&tmp).ok();
                fs::remove_file(&tar).ok();
                remote_version = vex_lang::get_values(&vex_lang::parse_vex(&content), "version")
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                cache::set_version(repo, pkg_name, &remote_version);
            }
            break;
        }
    }

    if remote_version.is_empty() {
        print::vex_error(&format!("'{}' not found in any repo", pkg_name));
        std::process::exit(1);
    }

    let local_version = Manifest::load(&manifest_path(pkg_name))
        .map(|m| m.version)
        .unwrap_or_default();

    if local_version == remote_version {
        print::vex_print("Up to date", &format!("{} ({})", pkg_name, local_version));
        return;
    }

    print::vex_print(
        "Upgrading",
        &format!("{}: {} -> {}", pkg_name, local_version, remote_version),
    );

    if let Err(e) = install_pkg(pkg_name, repos, true) {
        print::vex_error(&format!("upgrade failed: {}", e));
        std::process::exit(1);
    }

    print::vex_print("Upgraded", &format!("{} to {}", pkg_name, remote_version));
}
pub fn add_pkg(pkg_name: &str) {
    let path = format!("{}/.config/vex/pkgs.vex", home());
    let content = fs::read_to_string(&path).expect("Failed to read pkgs.vex");
    if content.contains(&format!("\"{}\"", pkg_name)) {
        print::vex_print("Already", &format!("{} is already in pkgs.vex", pkg_name));
        return;
    }
    let new_content =
        content.replacen("packages {", &format!("packages {{\n  \"{}\"", pkg_name), 1);
    fs::write(&path, new_content).expect("Failed to write pkgs.vex");
    print::vex_print("Added", &format!("{} — run vex sync to install", pkg_name));
}

pub fn remove_from_pkgs(pkg_name: &str) {
    let path = format!("{}/.config/vex/pkgs.vex", home());
    let content = fs::read_to_string(&path).expect("Failed to read pkgs.vex");
    if !content.contains(&format!("\"{}\"", pkg_name)) {
        print::vex_error(&format!("{} not found in pkgs.vex", pkg_name));
        return;
    }
    let new_content = content
        .lines()
        .filter(|l| l.trim() != format!("\"{}\"", pkg_name))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, new_content).expect("Failed to write pkgs.vex");
    print::vex_print(
        "Removed",
        &format!("{} — run vex sync to uninstall", pkg_name),
    );
}
// ── local tarball install ─────────────────────────────────────────────────────

pub fn build_tar(tar_name: &str, repos: &[String]) {
    let tmp_dir = format!("{}/.vex/.tmp_build_tar", home());
    if Path::new(&tmp_dir).exists() {
        fs::remove_dir_all(&tmp_dir).expect("Failed to clean stale tmp dir");
    }
    fs::create_dir_all(&tmp_dir).expect("Failed to create tmp extract dir");
    std::process::Command::new("tar")
        .args(["-xf", tar_name, "-C", &tmp_dir])
        .status()
        .unwrap_or_else(|e| {
            print::vex_error(&format!("failed to extract tarball: {}", e));
            std::process::exit(1);
        });
    let build_content =
        fs::read_to_string(format!("{}/build.vex", tmp_dir)).expect("Failed to read build.vex");
    let parsed = vex_lang::parse_vex(&build_content);
    let pkg_name = vex_lang::get_values(&parsed, "name")
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            print::vex_error("build.vex is missing a 'name' field");
            std::process::exit(1);
        });
    let version = vex_lang::get_values(&parsed, "version")
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            print::vex_error("build.vex is missing a 'version' field");
            std::process::exit(1);
        });
    for dep in vex_lang::get_values(&parsed, "dependencies") {
        if !dep.is_empty() {
            print::vex_print("Installing", &format!("dep {}", dep));
            if let Err(e) = install_pkg(&dep, repos, false) {
                print::vex_error(&format!("error installing dep '{}': {}", dep, e));
                let _ = fs::remove_dir_all(&tmp_dir);
                return;
            }
        }
    }
    let real_dir = pkg_dir(&pkg_name);
    if Path::new(&real_dir).exists() {
        fs::remove_dir_all(&real_dir).expect("Failed to wipe existing pkg dir");
    }
    fs::rename(&tmp_dir, &real_dir).expect("Failed to move pkg to pkgs dir");
    let roots = watch_roots();
    let before = snapshot_paths(&roots);
    for cmd in vex_lang::get_values(&parsed, "commands") {
        print::vex_print("Running", &cmd);
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&real_dir)
            .status()
            .expect("Failed to run command");
        if !status.success() {
            print::vex_error(&format!("command failed: {}", cmd));
            return;
        }
    }
    let after = snapshot_paths(&roots);
    let (new_dirs, new_files) = diff_snapshots(&before, &after, Some(Path::new(&real_dir)));
    let mut manifest = Manifest::new(version);
    for d in &new_dirs {
        manifest.record_dir(d);
    }
    for f in &new_files {
        manifest.record_file(f);
    }
    if let Err(e) = manifest.save(&manifest_path(&pkg_name)) {
        print::vex_warn(&format!("could not save manifest: {}", e));
    }
    print::vex_print("Installed", &format!("{} (from local tarball)", pkg_name));
}
