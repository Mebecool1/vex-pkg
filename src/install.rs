use crate::fetch;
use crate::vex_lang;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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
    format!("{}/{}.tar", vex_pkgs_dir(), pkg_name)
}

fn manifest_path(pkg_name: &str) -> String {
    format!("{}/.vex_manifest", pkg_dir(pkg_name))
}

/// A package is only considered properly installed if BOTH the dir AND manifest exist.
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
                            Ok(_) => println!("  [rm file] {}", p.display()),
                            Err(e) => eprintln!("  [warn] could not remove {}: {}", p.display(), e),
                        }
                    }
                }
                ManifestEntry::Dir(p) => {
                    if let Ok(mut rd) = fs::read_dir(p) {
                        if rd.next().is_none() {
                            match fs::remove_dir(p) {
                                Ok(_) => println!("  [rm dir] {}", p.display()),
                                Err(e) => eprintln!(
                                    "  [warn] could not remove dir {}: {}",
                                    p.display(),
                                    e
                                ),
                            }
                        } else {
                            println!("  [skip dir, not empty] {}", p.display());
                        }
                    }
                }
            }
        }
        // Remove the pkg source dir itself.
        if let Err(e) = fs::remove_dir_all(pkg_dir(pkg_name)) {
            eprintln!("  [warn] could not remove pkg dir: {}", e);
        } else {
            println!("  [rm pkg dir] {}", pkg_dir(pkg_name));
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

// ── fetch build.vex ───────────────────────────────────────────────────────────

fn fetch_build_vex(pkg_name: &str, repos: &[String]) -> Result<String, String> {
    // Prefer already-extracted build.vex.
    let installed_build = format!("{}/build.vex", pkg_dir(pkg_name));
    if let Ok(content) = fs::read_to_string(&installed_build) {
        return Ok(content);
    }

    let tar = tar_path(pkg_name);
    fs::create_dir_all(vex_pkgs_dir()).map_err(|e| e.to_string())?;

    let mut fetched = false;
    for repo in repos {
        if fetch::list_pkgs_from_url(repo).contains(&format!("{}.tar", pkg_name)) {
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

    // Promote tmp dir → real pkg dir.
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
// ── state machine: sync ───────────────────────────────────────────────────────

pub fn sync(desired_pkgs: &[String], repos: &[String]) {
    println!(
        "vex: resolving dependency closure for {} top-level package(s)...",
        desired_pkgs.len()
    );
    // -- get a version -------------------------------------------------------
    fn peek_version(pkg_name: &str, repos: &[String]) -> String {
        let tar = tar_path(pkg_name);
        fs::remove_file(&tar).ok();
        for repo in repos {
            if fetch::list_pkgs_from_url(repo).contains(&format!("{}.tar", pkg_name)) {
                fetch::fetch_pkg_to(repo, pkg_name, &tar);
                break;
            }
        }
        let tmp = format!("{}/.vex/.tmp_peek_{}", home(), pkg_name);
        fs::create_dir_all(&tmp).ok();
        std::process::Command::new("tar")
            .args(["-xf", &tar, "-C", &tmp])
            .status()
            .ok();
        let content = fs::read_to_string(format!("{}/build.vex", tmp)).unwrap_or_default();
        fs::remove_dir_all(&tmp).ok();
        fs::remove_file(&tar).ok();
        let version = vex_lang::get_values(&vex_lang::parse_vex(&content), "version")
            .into_iter()
            .next()
            .unwrap_or_default();
        println!(
            "newest version available of package {} is {}",
            pkg_name, version
        );
        version
    }

    // does it even need an update?
    fn needs_update(pkg_name: &str, repos: &[String]) -> bool {
        let local = Manifest::load(&manifest_path(pkg_name))
            .map(|m| m.version)
            .unwrap_or_default();
        let remote = peek_version(pkg_name, repos);
        local != remote
    }
    let mut visited = HashSet::new();
    let mut needed: HashSet<String> = HashSet::new();
    // ── fetch build.vex ───────────────────────────────────────────────────────────

    fn fetch_build_vex(pkg_name: &str, repos: &[String]) -> Result<String, String> {
        // Prefer already-extracted build.vex.
        let installed_build = format!("{}/build.vex", pkg_dir(pkg_name));
        if let Ok(content) = fs::read_to_string(&installed_build) {
            return Ok(content);
        }

        let tar = tar_path(pkg_name);
        fs::create_dir_all(vex_pkgs_dir()).map_err(|e| e.to_string())?;

        let mut fetched = false;
        for repo in repos {
            if fetch::list_pkgs_from_url(repo).contains(&format!("{}.tar", pkg_name)) {
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

        // Promote tmp dir → real pkg dir.
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

    // installed_pkgs() reads dirs — includes legacy installs without manifests.
    let currently_installed = installed_pkgs();

    // Anything in the closure that isn't PROPERLY installed (dir + manifest) or is outdated needs installing.

    let to_install: Vec<_> = needed
        .iter()
        .filter(|p| !is_installed(p) || needs_update(p, repos))
        .collect();
    let to_update: Vec<_> = needed.iter().filter(|p| needs_update(p, repos)).collect();
    println!("outdated things: {:?}", to_update);
    println!("things needed to be installed: {:?}", to_update);

    if to_install.is_empty() {
        println!("vex: nothing to install.");
    } else {
        println!("vex: installing {} package(s)...", to_install.len());
        for pkg in &to_install {
            if let Err(e) = install_pkg(pkg, repos, false) {
                eprintln!("error installing '{}': {}", pkg, e);
                eprintln!("aborting sync — some packages may be partially installed.");
                return;
            }
        }
        for pkg in &to_update {
            if let Err(e) = install_pkg(pkg, repos, true) {
                eprintln!("error updating '{}: {}", pkg, e);
                eprintln!("aborting sync - some packages may be partially installed.");
                return;
            }
        }
    }

    // Anything on disk (by dir) that isn't in the closure gets removed.
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
            remove_pkg(pkg, repos);
        }
    }

    println!("vex: sync complete. system matches desired state.");
}

// ── install ───────────────────────────────────────────────────────────────────

fn install_pkg(pkg_name: &str, repos: &[String], force: bool) -> Result<(), String> {
    // If a dir exists but no manifest, it's a dirty/legacy install — wipe and redo.
    let dir = pkg_dir(pkg_name);
    if Path::new(&dir).exists() && !Path::new(&manifest_path(pkg_name)).exists() {
        println!("  [reinstalling] {} (no manifest found)...", pkg_name);
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    } else if is_installed(pkg_name) && !force {
        println!("  [already installed] {}", pkg_name);
        return Ok(());
    } else {
        println!("  [installing] {}...", pkg_name);
    }

    // Fetch tarball if dir doesn't exist yet (fetch_build_vex may have already promoted it).
    if !Path::new(&dir).exists() {
        let tar = tar_path(pkg_name);
        fs::create_dir_all(vex_pkgs_dir()).map_err(|e| e.to_string())?;

        let mut fetched = false;
        for repo in repos {
            if fetch::list_pkgs_from_url(repo).contains(&format!("{}.tar", pkg_name)) {
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
    let version = vex_lang::get_values(&parsed, "version")
        .into_iter()
        .next()
        .expect("Missing version field in build.vex");
    let _name = vex_lang::get_values(&parsed, "name")
        .into_iter()
        .next()
        .expect("Missing name field in build.vex");

    // Snapshot before running build commands.
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

    // Build manifest from diff.
    let after = snapshot_paths(&roots);
    let (new_dirs, new_files) = diff_snapshots(&before, &after, Some(Path::new(&dir)));

    let mut manifest = Manifest::new((*version).to_string());
    for d in &new_dirs {
        manifest.record_dir(d);
    }
    for f in &new_files {
        manifest.record_file(f);
    }
    manifest.save(&manifest_path(pkg_name))?;

    println!(
        "  [manifest] tracked {} file(s), {} dir(s)",
        new_files.len(),
        new_dirs.len()
    );
    println!("  [installed] {}", pkg_name);
    Ok(())
}

// ── remove ────────────────────────────────────────────────────────────────────

fn remove_pkg(pkg_name: &str, _repos: &[String]) {
    println!("  [removing] {}...", pkg_name);

    match Manifest::load(&manifest_path(pkg_name)) {
        Ok(manifest) => {
            manifest.uninstall(pkg_name);
        }
        Err(_) => {
            eprintln!(
                "  [warn] no manifest for '{}' — removing pkg dir only.",
                pkg_name
            );
            eprintln!(
                "  [warn] files installed outside ~/.vex/pkgs/{} were NOT cleaned up.",
                pkg_name
            );
            let _ = fs::remove_dir_all(pkg_dir(pkg_name));
        }
    }

    println!("  [removed] {}", pkg_name);
}

// ── local tarball install ─────────────────────────────────────────────────────

pub fn build_tar(tar_name: &str, repos: &[String]) {
    // Peek at build.vex without committing to a final location yet.
    let tmp_dir = format!("{}/.vex/.tmp_build_tar", home());
    if Path::new(&tmp_dir).exists() {
        fs::remove_dir_all(&tmp_dir).expect("Failed to clean stale tmp dir");
    }
    fs::create_dir_all(&tmp_dir).expect("Failed to create tmp extract dir");

    std::process::Command::new("tar")
        .args(["-xf", tar_name, "-C", &tmp_dir])
        .status()
        .expect("Failed to extract tarball");

    let build_content =
        fs::read_to_string(format!("{}/build.vex", tmp_dir)).expect("Failed to read build.vex");
    let parsed = vex_lang::parse_vex(&build_content);

    // Get the package name from build.vex so we can place it correctly.
    let pkg_name = vex_lang::get_values(&parsed, "name")
        .into_iter()
        .next()
        .expect("build.vex is missing a 'name' field");
    let version = vex_lang::get_values(&parsed, "version")
        .into_iter()
        .next()
        .expect("build.vex misses a 'version' field");
    // Install dependencies before touching the filesystem snapshot.
    for dep in vex_lang::get_values(&parsed, "dependencies") {
        if !dep.is_empty() {
            println!("  [dep] installing: {}", dep);
            if let Err(e) = install_pkg(&dep, repos, false) {
                eprintln!("error installing dep '{}': {}", dep, e);
                let _ = fs::remove_dir_all(&tmp_dir);
                return;
            }
        }
    }

    // Move tmp → real pkg dir before running commands so CWD is stable
    // and is_installed() works correctly afterward.
    let real_dir = pkg_dir(&pkg_name);
    if Path::new(&real_dir).exists() {
        fs::remove_dir_all(&real_dir).expect("Failed to wipe existing pkg dir");
    }
    fs::rename(&tmp_dir, &real_dir).expect("Failed to move pkg to pkgs dir");

    let roots = watch_roots();
    let before = snapshot_paths(&roots);

    for cmd in vex_lang::get_values(&parsed, "commands") {
        println!("    $ {}", cmd);
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&real_dir) // run from the real pkg dir
            .status()
            .expect("Failed to run command");

        if !status.success() {
            eprintln!("  [error] command failed: {}", cmd);
            return;
        }
    }

    let after = snapshot_paths(&roots);
    // Exclude the pkg dir itself — we don't want build.vex etc. in the manifest.
    let (new_dirs, new_files) = diff_snapshots(&before, &after, Some(Path::new(&real_dir)));

    let mut manifest = Manifest::new(version);
    for d in &new_dirs {
        manifest.record_dir(d);
    }
    for f in &new_files {
        manifest.record_file(f);
    }
    if let Err(e) = manifest.save(&manifest_path(&pkg_name)) {
        eprintln!("[warn] could not save manifest: {}", e);
    } else {
        println!(
            "  [manifest] tracked {} file(s), {} dir(s)",
            new_files.len(),
            new_dirs.len()
        );
    }

    println!("  [installed] {} (from local tarball)", pkg_name);
}
