use colored::Colorize;

pub fn vex_print(verb: &str, msg: &str) {
    println!("{:>12} {}", verb.green().bold(), msg);
}

pub fn vex_error(msg: &str) {
    eprintln!("{:>12} {}", "Error".red().bold(), msg);
}

pub fn vex_warn(msg: &str) {
    eprintln!("{:>12} {}", "Warning".yellow().bold(), msg);
}
