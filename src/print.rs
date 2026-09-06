use colored::Colorize;

pub fn vex_print(verb: &str, msg: &str) {
    let colored_verb = match verb {
        "Removing" | "Removed" => verb.red().bold(),
        "Updating" => verb.yellow().bold(),
        "Fetching" | "Resolving" => verb.cyan().bold(),
        "Fresh" => verb.blue().bold(),
        "Finished" | "Installed" | "Synced" => verb.green().bold(),
        _ => verb.green().bold(),
    };
    println!("{:>12} {}", colored_verb, msg);
}

pub fn vex_error(msg: &str) {
    eprintln!("{:>12} {}", "Error".red().bold(), msg);
}

pub fn vex_warn(msg: &str) {
    eprintln!("{:>12} {}", "Warning".yellow().bold(), msg);
}

pub fn vex_print_no_ln(verb: &str, msg: &str) {
    let colored_verb = match verb {
        "Compiling" => verb.cyan().bold(),
        _ => verb.green().bold(),
    };
    print!("{:>12} {}", colored_verb, msg);
}
