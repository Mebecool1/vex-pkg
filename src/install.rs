use std::fs;
use std::env;
use crate::{vex_lang};
use crate::{fetch};

pub fn is_installed(pkg_name: &str) -> bool {
    let home = env::var("HOME").expect("Failed to get HOME");
    std::path::Path::new(&format!("{}/.vex/pkgs/{}", home, pkg_name)).exists()
}

pub fn rebuild(pkgs: &[String], repos: &[String]) {
    println!("rebuilding {} package(s)...", pkgs.len());
    for pkg in pkgs {
        install_pkg(pkg, repos);
    }
    println!("rebuilder: done!");
}

fn install_pkg(pkg_name: &str, repos: &[String]) {
    if is_installed(pkg_name) {
        println!("{} already installed, skipping", pkg_name);
        return;
    }

    println!("installing {}...", pkg_name);

    let home = env::var("HOME").expect("Failed to get HOME");
    let pkg_dir = format!("{}/.vex/pkgs/{}", home, pkg_name);
    let tar_path = format!("{}/.vex/pkgs/{}.tar", home, pkg_name);

    // create pkgs dir if it doesn't exist
    fs::create_dir_all(&format!("{}/.vex/pkgs", home)).expect("Failed to create vex pkgs dir");

    // fetch from first repo that has it
    let mut fetched = false;
    for repo in repos {
        let available = fetch::list_pkgs_from_url(repo);
        if available.contains(&format!("{}.tar", pkg_name)) {
            fetch::fetch_pkg_to(repo, pkg_name, &tar_path);
            fetched = true;
            break;
        }
    }

    if !fetched {
        eprintln!("error: {} not found in any repo", pkg_name);
        return;
    }

    // extract to ~/.vex/pkgs/pkgname/
    fs::create_dir_all(&pkg_dir).expect("Failed to create pkg dir");
    std::process::Command::new("tar")
        .args(["-xf", &tar_path, "-C", &pkg_dir])
        .status()
        .expect("Failed to extract tarball");

    fs::remove_file(&tar_path).ok();

    // parse build.vex
    let build_content = fs::read_to_string(format!("{}/build.vex", pkg_dir))
        .expect("Failed to read build.vex");
    let parsed_build = vex_lang::parse_vex(&build_content);

    // resolve deps recursively first
    let deps = vex_lang::get_values(&parsed_build, "dependencies");
    for dep in &deps {
        if !dep.is_empty() {
            install_pkg(dep, repos);
        }
    }

    // run commands
    let commands = vex_lang::get_values(&parsed_build, "commands");
    for cmd in &commands {
        println!("running: {}", cmd);
        std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&pkg_dir)
            .status()
            .expect("Failed to run command");
    }

    println!("{} installed!", pkg_name);
}