//! Identifier validation for reserved legacy addon credential namespaces.

pub fn validate_addon_id(addon_id: &str) -> Result<(), String> {
    if addon_id.is_empty() {
        return Err("Invalid addon id: id is empty".to_string());
    }
    if addon_id.len() > 64 {
        return Err("Invalid addon id: id must be 64 characters or fewer".to_string());
    }
    if addon_id == "." || addon_id == ".." || addon_id.chars().all(|c| c == '.') {
        return Err("Invalid addon id: dot-only ids are not allowed".to_string());
    }
    if addon_id == "staging" {
        return Err("Invalid addon id: 'staging' is reserved".to_string());
    }

    let mut chars = addon_id.chars();
    let Some(first) = chars.next() else {
        return Err("Invalid addon id: id is empty".to_string());
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err("Invalid addon id: id must start with a lowercase letter or digit".to_string());
    }

    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
    {
        return Err("Invalid addon id: use lowercase letters, digits, '.', '_' or '-'".to_string());
    }

    Ok(())
}
