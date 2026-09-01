use crate::fetch;
use crate::vex_lang;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// ── helpers ──────────────────────────────────────────────────────────────────

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
    format!("{}/{}.tar", vex_pkgs_dir(), pkg_name)
}

/// Path to the manifest file for a package.
/// Stored inside the pkg dir so it is co-located with the source tree.
fn manifest_path(pkg_name: &str) -> String {
    format!("{}/.vex_manifest", pkg_dir(pkg_name))
}

pub fn is_installed(pkg_name: &str) -> bool {
    Path::new(&pkg_dir(pkg_name)).exists()
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
//
// A manifest is a plain text file; one absolute path per line.
// Lines starting with 'F' are files, lines starting with 'D' are directories
// (recorded in creation order; reversed on removal so children come before parents).
//
// Format:
//   F /home/user/.vex/bin/mytool
//   D /home/user/.vex/bin
//   F /home/user/.local/lib/libfoo.so

#[derive(Debug)]
enum ManifestEntry {
    File(PathBuf),
    Dir(PathBuf),
}

struct Manifest {
    entries: Vec<ManifestEntry>,
}

impl Manifest {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn record_file(&mut self, path: impl Into<PathBuf>) {
        self.entries.push(ManifestEntry::File(path.into()));
    }

    fn record_dir(&mut self, path: impl Into<PathBuf>) {
        self.entries.push(ManifestEntry::Dir(path.into()));
    }

    fn save(&self, manifest_file: &str) -> Result<(), String> {
        let lines: Vec<String> = self
            .entries
            .iter()
            .map(|e| match e {
                ManifestEntry::File(p) => format!("F {}", p.display()),
                ManifestEntry::Dir(p) => format!("D {}", p.display()),
            })
            .collect();
        fs::write(manifest_file, lines.join("\n"))
            .map_err(|e| format!("Failed to write manifest: {}", e))
    }

    fn load(manifest_file: &str) -> Result<Self, String> {
        let content = fs::read_to_string(manifest_file)
            .map_err(|_| format!("No manifest found at {}", manifest_file))?;

        let entries = content
            .lines()
            .filter(|l| l.len() > 2)
            .map(|l| {
                let path = PathBuf::from(&l[2..]);
                match &l[..1] {
                    "F" => ManifestEntry::File(path),
                    "D" => ManifestEntry::Dir(path),
                    _ => ManifestEntry::File(path), // fallback
                }
            })
            .collect();

        Ok(Self { entries })
    }

    /// Remove everything the manifest recorded, in reverse order.
    /// Directories are only removed if they are empty after file removal.
    fn uninstall(&self, pkg_name: &str) {
        for entry in self.entries.iter().rev() {
            match entry {
                ManifestEntry::File(p) => {
                    if p.exists() {
                        if let Err(e) = fs::remove_file(p) {
                            eprintln!("  [warn] could not remove file {}: {}", p.display(), e);
                        } else {
                            println!("  [rm file] {}", p.display());
                        }
                    }
                }
                ManifestEntry::Dir(p) => {
                    // Only remove if now empty — another package may share this dir.
                    match fs::read_dir(p) {
                        Ok(mut rd) => {
                            if rd.next().is_none() {
                                if let Err(e) = fs::remove_dir(p) {
                                    eprintln!(
                                        "  [warn] could not remove dir {}: {}",
                                        p.display(),
                                        e
                                    );
                                } else {
                                    println!("  [rm dir] {}", p.display());
                                }
                            } else {
                                println!("  [skip dir, not empty] {}", p.display());
                            }
                        }
                        Err(_) => {} // already gone
                    }
                }
            }
        }
        // Finally remove the pkg source dir itself.
        let dir = pkg_dir(pkg_name);
        if let Err(e) = fs::remove_dir_all(&dir) {
            eprintln!("  [warn] could not remove pkg dir {}: {}", dir, e);
        } else {
            println!("  [rm pkg dir] {}", dir);
        }
    }
}

// ── filesystem snapshot diff ──────────────────────────────────────────────────
//
// Strategy: snapshot the set of all files+dirs under watched roots before the
// build commands run, then diff against after. Everything new goes into the manifest.
//
// Watched roots — anywhere a build.vex is reasonably likely to write to.
// We deliberately avoid watching / or /usr to keep this fast and safe.

fn watch_roots() -> Vec<PathBuf> {
    let h = home();
    vec![
        PathBuf::from(format!("{}/.vex", h)),
        PathBuf::from(format!("{}/.local", h)),
        PathBuf::from(format!("{}/.config", h)),
        PathBuf::from(format!("{}/.bin", h)),
        PathBuf::from(format!("{}/bin", h)),
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

/// Diff two snapshots and return new files and new dirs (in discovery order,
/// dirs before their children so we can record them for ordered teardown).
fn diff_snapshots(
    before: &HashSet<PathBuf>,
    after: &HashSet<PathBuf>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut new_dirs: Vec<PathBuf> = Vec::new();
    let mut new_files: Vec<PathBuf> = Vec::new();

    // Collect new dirs first (shortest path = parent before child).
    let mut new_paths: Vec<&PathBuf> = after.difference(before).collect();
    new_paths.sort_by_key(|p| p.components().count());

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
    needed: &mut HashSet<String>,
) -> Result<(), String> {
    if visited.contains(pkg_name) {
        return Ok(());
    }
    visited.insert(pkg_name.to_string());
    needed.insert(pkg_name.to_string());

    let build_content = fetch_build_vex(pkg_name, repos)?;
    let parsed = vex_lang::parse_vex(&build_content);

    for dep in vex_lang::get_values(&parsed, "dependencies") {
        if !dep.is_empty() {
            resolve_deps(&dep, repos, visited, needed)?;
        }
    }
    Ok(())
}

fn fetch_build_vex(pkg_name: &str, repos: &[String]) -> Result<String, String> {
    let installed_build = format!("{}/build.vex", pkg_dir(pkg_name));
    if let Ok(content) = fs::read_to_string(&installed_build) {
        return Ok(content);
    }

    let tar = tar_path(pkg_name);
    fs::create_dir_all(vex_pkgs_dir()).map_err(|e| e.to_string())?;

    let mut fetched = false;
    for repo in repos {
        let available = fetch::list_pkgs_from_url(repo);
        if available.contains(&format!("{}.tar", pkg_name)) {
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
    std::process::Command::new("tar")
        .args(["-xf", &tar, "-C", &tmp_dir])
        .status()
        .expect("Failed to extract tarball");
    fs::remove_file(&tar).ok();

    let content = fs::read_to_string(format!("{}/build.vex", tmp_dir))
        .map_err(|_| format!("build.vex missing in package '{}'", pkg_name))?;

    fs::rename(&tmp_dir, pkg_dir(pkg_name))
        .map_err(|e| format!("Failed to promote tmp dir: {}", e))?;

    Ok(content)
}

// ── state machine: sync ───────────────────────────────────────────────────────

pub fn sync(desired_pkgs: &[String], repos: &[String]) {
    println!(
        "vex: resolving dependency closure for {} top-level package(s)...",
        desired_pkgs.len()
    );

    let mut visited = HashSet::new();
    let mut needed: HashSet<String> = HashSet::new();

    for pkg in desired_pkgs {
        if let Err(e) = resolve_deps(pkg, repos, &mut visited, &mut needed) {
            eprintln!("error: {}", e);
            eprintln!("aborting sync — system state unchanged.");
            return;
        }
    }

    println!("vex: closure contains {} package(s): {:?}", needed.len(), {
        let mut v: Vec<_> = needed.iter().collect();
        v.sort();
        v
    });

    let currently_installed = installed_pkgs();

    // Install missing packages.
    let to_install: Vec<_> = needed
        .iter()
        .filter(|p| !currently_installed.contains(*p))
        .collect();

    if to_install.is_empty() {
        println!("vex: nothing to install.");
    } else {
        println!("vex: installing {} package(s)...", to_install.len());
        for pkg in &to_install {
            if let Err(e) = install_pkg(pkg, repos) {
                eprintln!("error installing '{}': {}", pkg, e);
                eprintln!("aborting sync — some packages may be partially installed.");
                return;
            }
        }
    }

    // Remove packages not in the closure.
    let to_remove: Vec<_> = currently_installed
        .iter()
        .filter(|p| !needed.contains(*p))
        .collect();

    if to_remove.is_empty() {
        println!("vex: nothing to remove.");
    } else {
        println!(
            "vex: removing {} package(s) not in desired set...",
            to_remove.len()
        );
        for pkg in &to_remove {
            remove_pkg(pkg);
        }
    }

    println!("vex: sync complete. system matches desired state.");
}

// ── install ───────────────────────────────────────────────────────────────────

fn install_pkg(pkg_name: &str, repos: &[String]) -> Result<(), String> {
    if is_installed(pkg_name) {
        println!("  [already installed] {}", pkg_name);
        return Ok(());
    }

    println!("  [installing] {}...", pkg_name);
    let dir = pkg_dir(pkg_name);
    let tar = tar_path(pkg_name);

    fs::create_dir_all(vex_pkgs_dir()).map_err(|e| e.to_string())?;

    if !Path::new(&dir).exists() {
        let mut fetched = false;
        for repo in repos {
            let available = fetch::list_pkgs_from_url(repo);
            if available.contains(&format!("{}.tar", pkg_name)) {
                fetch::fetch_pkg_to(repo, pkg_name, &tar);
                fetched = true;
                break;
            }
        }
        if !fetched {
            return Err(format!("'{}' not found in any repo", pkg_name));
        }

        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::process::Command::new("tar")
            .args(["-xf", &tar, "-C", &dir])
            .status()
            .expect("Failed to extract tarball");
        fs::remove_file(&tar).ok();
    }

    let build_content = fs::read_to_string(format!("{}/build.vex", dir))
        .map_err(|_| format!("build.vex missing for '{}'", pkg_name))?;
    let parsed = vex_lang::parse_vex(&build_content);
    let commands = vex_lang::get_values(&parsed, "commands");

    // Snapshot the filesystem before running any build commands.
    let roots = watch_roots();
    let before = snapshot_paths(&roots);

    for cmd in &commands {
        println!("    $ {}", cmd);
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&dir)
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

    // Snapshot after and build the manifest from the diff.
    let after = snapshot_paths(&roots);
    let (new_dirs, new_files) = diff_snapshots(&before, &after);

    let mut manifest = Manifest::new();
    // Record dirs in creation order (parent → child) so teardown can reverse to child → parent.
    for d in &new_dirs {
        manifest.record_dir(d);
    }
    for f in &new_files {
        manifest.record_file(f);
    }

    let mpath = manifest_path(pkg_name);
    manifest.save(&mpath)?;
    println!(
        "  [manifest] tracked {} file(s), {} dir(s)",
        new_files.len(),
        new_dirs.len()
    );
    println!("  [installed] {}", pkg_name);
    Ok(())
}

// ── remove ────────────────────────────────────────────────────────────────────

fn remove_pkg(pkg_name: &str) {
    println!("  [removing] {}...", pkg_name);

    let mpath = manifest_path(pkg_name);
    match Manifest::load(&mpath) {
        Ok(manifest) => {
            manifest.uninstall(pkg_name);
        }
        Err(e) => {
            // No manifest — package was installed before manifest support.
            // Fall back to just removing the pkg dir and warn loudly.
            eprintln!("  [warn] {}", e);
            eprintln!(
                "  [warn] no manifest for '{}'; only pkg dir will be removed.",
                pkg_name
            );
            eprintln!(
                "  [warn] files outside ~/.vex/pkgs/{} were NOT cleaned up.",
                pkg_name
            );
            let dir = pkg_dir(pkg_name);
            if let Err(e) = fs::remove_dir_all(&dir) {
                eprintln!("  [error removing {}]: {}", pkg_name, e);
            }
        }
    }

    println!("  [removed] {}", pkg_name);
}

// ── local tarball install ─────────────────────────────────────────────────────

pub fn build_tar(tar_name: &str, repos: &[String]) {
    let extract_dir = format!("{}_dir", tar_name);
    fs::create_dir_all(&extract_dir).expect("Failed to create extract dir");

    std::process::Command::new("tar")
        .args(["-xf", tar_name, "-C", &extract_dir])
        .status()
        .expect("Failed to extract tarball");

    let build_content =
        fs::read_to_string(format!("{}/build.vex", extract_dir)).expect("Failed to read build.vex");
    let parsed = vex_lang::parse_vex(&build_content);

    for dep in vex_lang::get_values(&parsed, "dependencies") {
        if !dep.is_empty() {
            println!("installing dependency: {}", dep);
            if let Err(e) = install_pkg(&dep, repos) {
                eprintln!("error installing dep '{}': {}", dep, e);
                return;
            }
        }
    }

    // Snapshot before build commands.
    let roots = watch_roots();
    let before = snapshot_paths(&roots);

    for cmd in vex_lang::get_values(&parsed, "commands") {
        println!("running: {}", cmd);
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&extract_dir)
            .status()
            .expect("Failed to run command");

        if !status.success() {
            eprintln!("command failed: {}", cmd);
            return;
        }
    }

    // Save manifest next to the extracted dir.
    let after = snapshot_paths(&roots);
    let (new_dirs, new_files) = diff_snapshots(&before, &after);
    let mut manifest = Manifest::new();
    for d in &new_dirs {
        manifest.record_dir(d);
    }
    for f in &new_files {
        manifest.record_file(f);
    }
    let mpath = format!("{}/.vex_manifest", extract_dir);
    if let Err(e) = manifest.save(&mpath) {
        eprintln!("[warn] could not save manifest: {}", e);
    }

    println!("{} installed!", tar_name);
}
