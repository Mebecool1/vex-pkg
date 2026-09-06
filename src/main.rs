use std::env;
use std::fs;
mod cache;
mod fetch;
mod install;
mod lockfile;
mod print;
mod serve;
mod vex_lang;
use colored::Colorize;
// start
fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("usage: vex <command>");
        eprintln!(
            "commands: sync, serve, list, version, fetch, build, postserve, help, exists, search, refresh, add, remove"
        );
        std::process::exit(1);
    }

    let home = env::var("HOME").expect("Failed to get HOME environment variable");
    let pkgs_path = format!("{}/.config/vex/pkgs.vex", home);
    let config_path = format!("{}/.config/vex/config.vex", home);

    let pkgs_content = fs::read_to_string(&pkgs_path).unwrap_or_else(|_| {
        let default = "packages {\n}\n".to_string();
        fs::create_dir_all(format!("{}/.config/vex", home)).ok();
        fs::write(&pkgs_path, &default).ok();
        default
    });

    let config_content = fs::read_to_string(&config_path).unwrap_or_else(|_| {
        let default = "repositories {\n}\nrefresh-time {\n  \"24\"\n}\n".to_string();
        fs::create_dir_all(format!("{}/.config/vex", home)).ok();
        fs::write(&config_path, &default).ok();
        default
    });
    let parsed_pkgs = vex_lang::parse_vex(&pkgs_content);
    let parsed_config = vex_lang::parse_vex(&config_content);
    let is_local_vc = vex_lang::get_values(&parsed_config, "local");
    let mut is_local = false;
    if is_local_vc.contains(&String::from("true")) {
        is_local = true;
    }
    let repos = vex_lang::get_values(&parsed_config, "repositories");
    let ttl = vex_lang::get_values(&parsed_config, "refresh-time")[0]
        .parse()
        .expect("Please input valid number in config.vex: refresh-time.");
    let pkgs = vex_lang::get_values(&parsed_pkgs, "packages");
    print::vex_print("Loaded", "configs and packages");
    match args[1].as_str() {
        "portserve" => {
            if args.len() < 3 {
                eprintln!("usage: vex portserve <port: 5 digit no.>");
                std::process::exit(1);
            }
            let port: u16 = args[2].parse().unwrap();
            if is_local {
                serve::serve(port, "127.0.0.1");
            } else {
                serve::serve(port, "0.0.0.0");
            }
        }
        "serve" => {
            if is_local {
                serve::serve(45311, "127.0.0.1");
            } else {
                serve::serve(45311, "0.0.0.0");
            }
        }
        "list" => {
            for repo in &repos {
                let pkgs_from_repo = fetch::list_pkgs_from_url(repo);
                print::vex_print("List", &format!("of pkgs: {:#?}", pkgs_from_repo))
            }
        }
        "fetch" => {
            if args.len() < 3 {
                eprintln!("usage: vex fetch <package_name>");
                std::process::exit(1);
            }
            let pkg_name = &args[2];
            for repo in &repos {
                fetch::fetch_pkg_to(repo, pkg_name, &format!("{}_pkg.tar", pkg_name));
            }
        }
        "sync" => {
            print::vex_print("Syncing", "packages incrementally");
            let locked = args.contains(&"--locked".to_string());
            install::sync(&pkgs, &repos, ttl, locked);
        }
        "version" => {
            println!(
                "    vex-pkg v1.0.0 ({} {}) — {} packages installed",
                std::env::consts::OS,
                std::env::consts::ARCH,
                install::get_installed_pkgs().len()
            );
        }
        "build" => {
            if args.len() < 3 {
                eprintln!("usage: vex build <name>.tar");
                std::process::exit(1);
            }
            let pkg = &args[2];
            println!("building package..");
            install::build_tar(pkg, &repos);
            println!("package built.")
        }
        "exists" => {
            if args.len() < 3 {
                eprintln!("usage: vex exists <name>");
                std::process::exit(1);
            }
            let pkg = &args[2];
            println!("searching for package..");
            let mut list: Vec<String> = Vec::new();
            for repo in &repos {
                list.extend(fetch::list_pkgs_from_url(repo));
            }

            if list
                .iter()
                .any(|p| p.trim_end_matches(".tar") == pkg.as_str())
            {
                println!(
                    "{} was found. You can edit the pkgs.vex if you want to install it.",
                    pkg
                );
            } else {
                print::vex_error(&format!(
                    "{} was not found. please check name or add repo.",
                    pkg
                ));
                std::process::exit(1);
            }
        }
        "search" => {
            if args.len() < 3 {
                eprintln!("please provide the search argument.");
                std::process::exit(1);
            }
            let query = &args[2];
            println!("searching for {}", query);
            for repo in &repos {
                for pkg in fetch::list_pkgs_from_url(repo) {
                    let name = pkg.trim_end_matches(".tar");
                    if name.contains(query.as_str()) {
                        println!("{} ({})", name, repo);
                    }
                }
            }
        }
        "info" => {
            if args.len() < 3 {
                eprintln!("Please provide info argument.");
                std::process::exit(1);
            }
            install::info(&args[2], &repos);
        }
        "refresh" => {
            cache::invalidate_all();
            print::vex_print("Refreshed", "cache");
        }
        "upgrade" => {
            if args.len() < 3 {
                print::vex_error("Please input a pkg");
                std::process::exit(1);
            }
            install::upgrade(&args[2], &repos, ttl);
            print::vex_print("Upgraded", &args[2]);
        }
        "reinstall" => {
            if args.len() < 3 {
                eprintln!("usage: vex reinstall <package_name>");
                std::process::exit(1);
            }
            if let Err(e) = install::install_pkg(&args[2], &repos, true) {
                print::vex_error(&format!("reinstall failed: {}", e));
                std::process::exit(1);
            }
            print::vex_print("Reinstalled", &args[2]);
        }
        "add" => {
            if args.len() < 3 {
                eprintln!("usage: vex add <package_name>");
                std::process::exit(1);
            }
            install::add_pkg(&args[2]);
        }
        "remove" => {
            if args.len() < 3 {
                eprintln!("usage: vex remove <package_name>");
                std::process::exit(1);
            }
            install::remove_from_pkgs(&args[2]);
        }
        "outdated" => {
            for pkg in &pkgs {
                if !install::is_installed(pkg) {
                    continue;
                }
                let local = install::local_version(pkg);
                let remote = install::remote_version(pkg, &repos, ttl);
                if local != remote {
                    println!("{}: {} -> {}", pkg, local, remote);
                }
            }
        }
        "specificupdate" => {
            if args.len() < 3 {
                eprintln!("usage: vex specificupdate <package_name>");
                std::process::exit(1);
            }
            print::vex_warn(
                "updating a single package may cause dependency version mismatches — run vex sync after if things break",
            );
            install::upgrade(&args[2], &repos, ttl);
        }
        "list-installed" => {
            for pkg in install::get_installed_pkgs() {
                println!("       {}", pkg.red().bold());
            }
        }
        "help" => {
            println!(
                "
                

Commands: {{

  serve -> serve a local server at localhost:45311
  fetch <pkg> -> fetch pkg
  build <pkg>.tar -> build pkg locally 
  sync -> sync from pkgs.vex from repos on config.vex
  list -> list available packages for install from all repos 
  version -> version 
  portserve <port> -> serve on specific port
  exists <name> -> check if a package exists in your repos
  search <name> -> search for a specific package (contains search)
  refresh -> invalidate all cache 
  add/remove <pkg> -> add or remove a pkg from the pkgs.vex config. vex sync is manually required afterward.
}}

vex_lang syntax: (.vex) {{
  
  key {{
    \"entry\"
    \"entry0\"
    \"entry1\"
  }}
  key0 {{
    \"entry\"
    \"entry0\"
    \"entry1\"
  }}
  ...

}}

example config.vex :

```
repos {{
  \"http://localhost:45311\"
  \"http://foo.some_server.bar\"
}}
```

example pkgs.vex :
```
packages {{
  \"neovim\"
  \"xenon\"
  \"foo\"
  \"bar\"
}}
```

all .tar within current directory will be served on portserve and serve.

the .tar must have build.vex within their immediate root.


example :

foo.tar / 
  
  build.vex 
  src/
    main.rs 
  Cargo.toml 
  Cargo.lock
  install.sh*
  .gitignore

example build.vex :
```

name {{
  \"sample\"
}}

version {{
  \"0.1.0\"
}}

dependencies {{
  \"xenon\"
  \"neovim\"
}}

commands {{
  \"echo installing\"
  \"cargo build\"
  \"./install.sh\"
}}

```
must also have pkgs.list. example :
```
xenon v5.5.5-vex
lua v5.4.7-vex
tcc v0.9.5
```
do not ask queries. I do not have enough free time. This is a hobby project for all.

            "
            )
        }
        cmd => {
            eprintln!("unknown command: {}", cmd);
            std::process::exit(1);
        }
    }
}
