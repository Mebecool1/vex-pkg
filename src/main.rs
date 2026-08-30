use std::env;
use std::fs;
mod fetch;
mod install;
mod serve;
mod vex_lang;
// start
fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("usage: vex <command>");
        eprintln!("commands: rebuild, serve, list, version, fetch, build, postserve, help");
        std::process::exit(1);
    }

    let home = env::var("HOME").expect("Failed to get HOME environment variable");
    let pkgs_content = fs::read_to_string(format!("{}/.config/vex/pkgs.vex", home))
        .expect("Failed to read pkgs.vex");
    let config_content = fs::read_to_string(format!("{}/.config/vex/config.vex", home))
        .expect("Failed to read config.vex");

    let parsed_pkgs = vex_lang::parse_vex(&pkgs_content);
    let parsed_config = vex_lang::parse_vex(&config_content);

    let repos = vex_lang::get_values(&parsed_config, "repositories");
    let pkgs = vex_lang::get_values(&parsed_pkgs, "packages");
    println!("Loaded all packages and repos...;");
    match args[1].as_str() {
        "portserve" => {
            if args.len() < 3 {
                eprintln!("usage: vex portserve <port: 5 digit no.>");
                std::process::exit(1);
            }
            let port: u16 = args[2].parse().unwrap();
            serve::serve(port);
        }
        "serve" => {
            serve::serve(45311);
        }
        "list" => {
            for repo in &repos {
                let pkgs_from_repo = fetch::list_pkgs_from_url(repo);
                println!("Packages from {}: {:?}", repo, pkgs_from_repo);
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
        "rebuild" => {
            println!("rebuilding packages incrementally...");
            install::rebuild(&pkgs, &repos);
            println!("packages rebuilt.");
        }
        "version" => {
            println!("vex-pkg v0.3.6")
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
        "help" => {
            println!(
                "
                

Commands: {{

  serve -> serve a local server at localhost:45311
  fetch <pkg> -> fetch pkg
  build <pkg>.tar -> build pkg locally 
  rebuild -> rebuild from pkgs.vex from repos on config.vex
  list -> list available packages for install from all repos 
  version -> version 
  portserve <port> -> serve on specific port 
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
// ignored and optional
name {{
  \"sample\"
}}
// ignored and optional 
version {{
  \"0.1.0\"
}}
// optional if you have none but not ignored 
dependencies {{
  \"xenon\"
  \"neovim\"
}}
// mandatory and not ignored
commands {{
  \"echo installing\"
  \"cargo build\"
  \"./install.sh\"
}}

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
