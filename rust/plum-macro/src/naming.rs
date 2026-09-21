/// Convert snake_case to camelCase: "set_filter" → "setFilter", "last_error" → "lastError".
pub fn snake_to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Given a model struct name, return the generated wasm class name.
/// "Todos" → "WasmTodos"
pub fn wasm_class_name(model_name: &str) -> String {
    format!("Wasm{}", model_name)
}

/// Convert PascalCase to kebab-case: "MyModel" → "my-model", "Todos" → "todos".
pub fn kebab_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

/// Convert PascalCase to snake_case: "MyModel" → "my_model", "Todos" → "todos".
pub fn pascal_to_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

/// Convert snake_case to PascalCase: "last_error" → "LastError", "list" → "List".
pub fn pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Lowercase the first character: "Todos" → "todos", "lastError" → "lastError".
pub fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}
