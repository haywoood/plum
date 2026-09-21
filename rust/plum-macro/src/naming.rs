/// "set_filter" -> "setFilter". Input that is already camelCase is kept.
pub fn snake_to_camel(s: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in s.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// "last_error" -> "LastError", "Todos" -> "Todos".
pub fn pascal_case(s: &str) -> String {
    let camel = snake_to_camel(s);
    let mut chars = camel.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => camel,
    }
}

/// "Todos" -> "todos", "lastError" -> "lastError".
pub fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// "MyModel" -> "my_model".
pub fn snake_case(s: &str) -> String {
    separated(s, '_')
}

/// "MyModel" -> "my-model".
pub fn kebab_case(s: &str) -> String {
    separated(s, '-')
}

fn separated(pascal: &str, separator: char) -> String {
    let mut out = String::new();
    for (i, c) in pascal.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(separator);
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// The generated wasm-bindgen class for a model: "Todos" -> "WasmTodos".
pub fn wasm_class_name(model: &str) -> String {
    format!("Wasm{model}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(snake_to_camel("set_filter"), "setFilter");
        assert_eq!(pascal_case("last_error"), "LastError");
        assert_eq!(pascal_case("Todos"), "Todos");
        assert_eq!(lower_first("Todos"), "todos");
        assert_eq!(snake_case("MyModel"), "my_model");
        assert_eq!(kebab_case("MyModel"), "my-model");
        assert_eq!(wasm_class_name("Todos"), "WasmTodos");
    }
}
