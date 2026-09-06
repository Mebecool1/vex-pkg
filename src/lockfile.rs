use std::collections::HashMap;
use std::env;
use std::fs;

fn home() -> String {
    env::var("HOME").expect("Failed to get HOME")
}

pub fn lock_path() -> String {
    format!("{}/.config/vex/lock.vex", home())
}

pub fn read() -> HashMap<String, String> {
    let content = fs::read_to_string(lock_path()).unwrap_or_default();
    let mut map = HashMap::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() == 2 {
            map.insert(parts[0].to_string(), parts[1].to_string());
        }
    }
    map
}

pub fn write(packages: &HashMap<String, String>) {
    let mut lines: Vec<String> = packages
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();
    lines.sort();
    fs::write(lock_path(), lines.join("\n")).ok();
}

pub fn update(pkg_name: &str, version: &str) {
    let mut lock = read();
    lock.insert(pkg_name.to_string(), version.to_string());
    write(&lock);
}

pub fn remove(pkg_name: &str) {
    let mut lock = read();
    lock.remove(pkg_name);
    write(&lock);
}
