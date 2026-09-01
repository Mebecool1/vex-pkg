use crate::fetch;
use crate::vex_lang;
use std::collections::HashSet;
use std::env;
use std::fs;

// ── helpers ──────────────────────────────────────────────────────────────────

fn vex_pkgs_dir() -> String {
    let home = env::var("HOME").expect("Failed to get HOME");
    format!("{}/.vex/pkgs", home)
}

fn pkg_dir(pkg_name: &str) -> String {
    format!("{}/{}", vex_pkgs_dir(), pkg_name)
}

fn tar_path(pkg_name: &str) -> String {
    format!("{}/{}.tar", vex_pkgs_dir(), pkg_name)
}

pub fn is_installed(pkg_name: &str) -> bool {
    std::path::Path::new(&pkg_dir(pkg_name)).exists()
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

// ── dependency resolution ─────────────────────────────────────────────────────

/// Recursively collect the full closure of dependencies for `pkg_name`.
/// Reads build.vex from already-fetched tarballs or installed packages.
/// Returns an error string if a package can't be found in any repo.
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
    let deps = vex_lang::get_values(&parsed, "dependencies");

    for dep in deps {
        if !dep.is_empty() {
            resolve_deps(&dep, repos, visited, needed)?;
        }
    }
    Ok(())
}

/// Returns the build.vex contents for a package, preferring the already-installed
/// copy and falling back to fetching a tarball temporarily.
fn fetch_build_vex(pkg_name: &str, repos: &[String]) -> Result<String, String> {
    let installed_build = format!("{}/build.vex", pkg_dir(pkg_name));
    if let Ok(content) = fs::read_to_string(&installed_build) {
        return Ok(content);
    }

    // Not installed — fetch the tarball, read build.vex, then remove the tarball.
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

    // Extract just enough to read build.vex.
    let tmp_dir = format!("{}/.vex/.tmp_{}", env::var("HOME").unwrap(), pkg_name);
    fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    std::process::Command::new("tar")
        .args(["-xf", &tar, "-C", &tmp_dir])
        .status()
        .expect("Failed to extract tarball");
    fs::remove_file(&tar).ok();

    let build_path = format!("{}/build.vex", tmp_dir);
    let content = fs::read_to_string(&build_path)
        .map_err(|_| format!("build.vex missing in package '{}'", pkg_name))?;

    // Promote the tmp dir to the real pkg dir so we don't re-fetch later.
    fs::rename(&tmp_dir, pkg_dir(pkg_name))
        .map_err(|e| format!("Failed to promote tmp dir: {}", e))?;

    Ok(content)
}

// ── state machine: sync ───────────────────────────────────────────────────────

/// Declarative sync: given a desired set of top-level packages and repos,
/// bring the system to exactly that state:
///   1. Resolve the full dependency closure of the desired set.
///   2. Install any package in the closure that isn't already installed.
///   3. Remove any installed package that isn't in the closure.
pub fn sync(desired_pkgs: &[String], repos: &[String]) {
    println!(
        "vex: resolving dependency closure for {} top-level package(s)...",
        desired_pkgs.len()
    );

    // Step 1 — resolve full closure.
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

    // Step 2 — install missing packages.
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

    // Step 3 — remove packages not in the closure.
    let to_remove: Vec<_> = currently_installed
        .iter()
        .filter(|p| !needed.contains(*p))
        .collect();

    if to_remove.is_empty() {
        println!("nothing to remove.");
    } else {
        println!("removing {} package(s) not in pkgs.vex...", to_remove.len());
        for pkg in &to_remove {
            remove_pkg(pkg);
        }
    }

    println!("rebuilder: done!");
}

// ── install / remove ──────────────────────────────────────────────────────────

fn install_pkg(pkg_name: &str, repos: &[String]) -> Result<(), String> {
    if is_installed(pkg_name) {
        println!("  [already installed] {}", pkg_name);
        return Ok(());
    }

    println!("  [installing] {}...", pkg_name);
    let dir = pkg_dir(pkg_name);
    let tar = tar_path(pkg_name);

    fs::create_dir_all(vex_pkgs_dir()).map_err(|e| e.to_string())?;

    // Fetch tarball (skip if the dir was promoted by fetch_build_vex earlier).
    if !std::path::Path::new(&dir).exists() {
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

    // Run build commands.
    let build_content = fs::read_to_string(format!("{}/build.vex", dir))
        .map_err(|_| format!("build.vex missing for '{}'", pkg_name))?;
    let parsed = vex_lang::parse_vex(&build_content);

    for cmd in vex_lang::get_values(&parsed, "commands") {
        println!("    $ {}", cmd);
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
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

    println!("  [installed] {}", pkg_name);
    Ok(())
}

fn remove_pkg(pkg_name: &str) {
    let dir = pkg_dir(pkg_name);
    match fs::remove_dir_all(&dir) {
        Ok(_) => println!("  [removed] {}", pkg_name),
        Err(e) => eprintln!("  [error removing {}]: {}", pkg_name, e),
    }
}

// ── local tarball install ─────────────────────────────────────────────────────

/// Install from a local .tar file (e.g. `vex install ./mypkg.tar`).
/// This does NOT remove other packages — use `sync` for full state management.
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

    let deps = vex_lang::get_values(&parsed, "dependencies");
    for dep in &deps {
        if !dep.is_empty() {
            println!("installing dependency: {}", dep);
            if let Err(e) = install_pkg(dep, repos) {
                eprintln!("error installing dep '{}': {}", dep, e);
                return;
            }
        }
    }

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

    println!("{} installed!", tar_name);
}
