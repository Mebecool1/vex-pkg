use std::collections::HashMap;

pub fn parse_vex(input: &str) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    let mut current_key: Option<String> = None;
    let mut current_values: Vec<String> = Vec::new();

    for line in input.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        if line == "}" {
            if let Some(key) = current_key.take() {
                map.insert(key, current_values.clone());
                current_values.clear();
            }
        } else if line.starts_with('"') {
            let value = line.trim_matches('"').to_string();
            current_values.push(value);
        } else {
            // it's a key (with or without the `{` on the same line)
            current_key = Some(line.trim_end_matches('{').trim().to_string());
        }
    }

    map
}

pub fn get_values(parsed_config: &HashMap<String, Vec<String>>, key: &str) -> Vec<String> {
    parsed_config.get(key).cloned().unwrap_or_default()
}
