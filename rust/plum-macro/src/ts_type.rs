//! Infer a TypeScript type string from a Rust type.
//!
//! Strips signal wrappers (RwSignal, Memo, ReadSignal, Signal) then maps
//! the inner type to its TypeScript equivalent.

use syn::Type;

/// Infer a TS type string from a Rust type (including signal wrappers).
pub fn infer_ts_type(ty: &Type) -> String {
    let inner = strip_signal_wrapper(ty);
    infer_ts_type_inner(inner)
}

/// Strip one layer of signal wrapper if present.
fn strip_signal_wrapper(ty: &Type) -> &Type {
    if let Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            let name = seg.ident.to_string();
            if matches!(name.as_str(), "RwSignal" | "Memo" | "ReadSignal" | "Signal") {
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return inner;
                    }
                }
            }
        }
    }
    ty
}

fn infer_ts_type_inner(ty: &Type) -> String {
    if let Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            let name = seg.ident.to_string();
            return match name.as_str() {
                "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize"
                | "f32" | "f64" => "number".to_string(),
                "bool" => "boolean".to_string(),
                "String" | "str" => "string".to_string(),
                "Vec" => {
                    if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                        if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                            return format!("{}[]", infer_ts_type_inner(inner));
                        }
                    }
                    "unknown[]".to_string()
                }
                "Option" => {
                    if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                        if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                            return format!("{} | null", infer_ts_type_inner(inner));
                        }
                    }
                    "unknown | null".to_string()
                }
                // Any other named type passes through (user declares it in types.ts)
                _ => name,
            };
        }
    }
    "unknown".to_string()
}

/// Returns `Some(error_message)` if the type is not supported as a watch field type.
/// Supported: primitives, `Vec<T>`, `Option<T>`, and simple named types (structs/enums).
pub fn validate_watch_type(ty: &Type) -> Option<String> {
    let inner = strip_signal_wrapper(ty);
    validate_watch_type_inner(inner)
}

fn validate_watch_type_inner(ty: &Type) -> Option<String> {
    match ty {
        Type::Reference(_) => Some("references are not supported as watch field types".into()),
        Type::Tuple(_) => Some("tuples are not supported as watch field types".into()),
        Type::Path(tp) => {
            if let Some(seg) = tp.path.segments.last() {
                let name = seg.ident.to_string();
                let is_primitive = matches!(
                    name.as_str(),
                    "i8" | "i16"
                        | "i32"
                        | "i64"
                        | "isize"
                        | "u8"
                        | "u16"
                        | "u32"
                        | "u64"
                        | "usize"
                        | "f32"
                        | "f64"
                        | "bool"
                        | "String"
                        | "str"
                );
                if is_primitive {
                    return None;
                }
                match &seg.arguments {
                    syn::PathArguments::None => None, // simple named type (user type)
                    syn::PathArguments::AngleBracketed(args) => {
                        if matches!(name.as_str(), "Vec" | "Option") {
                            if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                                return validate_watch_type_inner(inner);
                            }
                            None
                        } else {
                            Some(format!(
                                "generic type `{}<...>` is not supported as a watch field type (only Vec and Option are supported containers)",
                                name
                            ))
                        }
                    }
                    _ => Some(format!("type `{}` with unsupported arguments", name)),
                }
            } else {
                Some("empty type path is not supported".into())
            }
        }
        _ => Some("this type form is not supported".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn primitives() {
        assert_eq!(infer_ts_type(&parse_quote!(i32)), "number");
        assert_eq!(infer_ts_type(&parse_quote!(bool)), "boolean");
        assert_eq!(infer_ts_type(&parse_quote!(String)), "string");
        assert_eq!(infer_ts_type(&parse_quote!(f64)), "number");
    }

    #[test]
    fn signal_wrappers() {
        assert_eq!(infer_ts_type(&parse_quote!(RwSignal<i32>)), "number");
        assert_eq!(infer_ts_type(&parse_quote!(Memo<bool>)), "boolean");
        assert_eq!(infer_ts_type(&parse_quote!(ReadSignal<String>)), "string");
    }

    #[test]
    fn collections() {
        assert_eq!(infer_ts_type(&parse_quote!(RwSignal<Vec<Todo>>)), "Todo[]");
        assert_eq!(infer_ts_type(&parse_quote!(Memo<Vec<Todo>>)), "Todo[]");
        assert_eq!(
            infer_ts_type(&parse_quote!(Memo<Option<String>>)),
            "string | null"
        );
    }

    #[test]
    fn pass_through() {
        assert_eq!(infer_ts_type(&parse_quote!(Filter)), "Filter");
        assert_eq!(infer_ts_type(&parse_quote!(RwSignal<Filter>)), "Filter");
    }

    #[test]
    fn validate_supported() {
        assert!(validate_watch_type(&parse_quote!(i32)).is_none());
        assert!(validate_watch_type(&parse_quote!(bool)).is_none());
        assert!(validate_watch_type(&parse_quote!(String)).is_none());
        assert!(validate_watch_type(&parse_quote!(Todo)).is_none());
        assert!(validate_watch_type(&parse_quote!(RwSignal<Vec<Todo>>)).is_none());
        assert!(validate_watch_type(&parse_quote!(Memo<Option<String>>)).is_none());
        assert!(validate_watch_type(&parse_quote!(Vec<i32>)).is_none());
    }

    #[test]
    fn validate_unsupported() {
        assert!(validate_watch_type(&parse_quote!(&str)).is_some());
        assert!(validate_watch_type(&parse_quote!((i32, String))).is_some());
        assert!(validate_watch_type(&parse_quote!(HashMap<String, i32>)).is_some());
        assert!(validate_watch_type(&parse_quote!(RwSignal<HashMap<String, i32>>)).is_some());
    }
}
