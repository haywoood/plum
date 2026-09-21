//! The `PlumModel` derive macro: generates a `#[wasm_bindgen]` wrapper struct
//! with watch methods, unwatch, dispose, and a `new_with` constructor.
//! Also writes a `.ts` binding file to `$CARGO_MANIFEST_DIR/plum_gen/`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

use crate::naming::{
    kebab_case, lower_first, pascal_case, pascal_to_snake, snake_to_camel, wasm_class_name,
};
use crate::ts_type::{infer_ts_type, validate_watch_type};

/// A watched field's metadata collected from the struct.
struct WatchInfo {
    /// Field name (used to call the core accessor).
    field_name: String,
    /// JS method name for the watch (e.g. "watchTodos").
    js_name: String,
    /// Store key in the generated TS (lowerCamelCase JS-facing name).
    store_key: String,
    /// Inferred TypeScript type (e.g. "Todo[]").
    ts_type: String,
}

pub fn expand(input: &DeriveInput) -> TokenStream {
    let struct_name = &input.ident;
    let struct_name_str = struct_name.to_string();
    let wasm_name_str = wasm_class_name(&struct_name_str);
    let wasm_name_ident = syn::Ident::new(&wasm_name_str, proc_macro2::Span::call_site());

    // Parse struct-level plum config (reserved for future use)
    let _ = &input.attrs;

    let data_struct = match &input.data {
        Data::Struct(s) => s,
        _ => {
            return syn::Error::new_spanned(struct_name, "PlumModel can only be derived on structs")
                .to_compile_error()
        }
    };

    let fields = match &data_struct.fields {
        Fields::Named(f) => &f.named,
        _ => {
            return syn::Error::new_spanned(
                struct_name,
                "PlumModel requires a struct with named fields",
            )
            .to_compile_error()
        }
    };

    // Collect watched fields
    let mut watches: Vec<WatchInfo> = Vec::new();
    let mut type_errors: Vec<String> = Vec::new();
    for field in fields.iter() {
        let field_name = field.ident.as_ref().unwrap().to_string();
        let plum_attr = field.attrs.iter().find(|a| a.path().is_ident("plum"));
        if let Some(attr) = plum_attr {
            if let Some(js_override) = parse_watch_attr(attr) {
                if let Some(err) = validate_watch_type(&field.ty) {
                    type_errors.push(format!("field `{}`: {}", field_name, err));
                }
                let ts_type = infer_ts_type(&field.ty);
                let js_name = match js_override {
                    Some(ref suffix) => format!("watch{}", pascal_case(suffix)),
                    None => format!("watch{}", pascal_case(&field_name)),
                };
                let store_key = match js_override {
                    Some(ref suffix) => lower_first(suffix),
                    None => snake_to_camel(&field_name),
                };
                watches.push(WatchInfo {
                    field_name,
                    js_name,
                    store_key,
                    ts_type,
                });
            }
        }
    }
    if !type_errors.is_empty() {
        return syn::Error::new_spanned(
            struct_name,
            format!(
                "plum: unsupported watch field types: {}",
                type_errors.join("; ")
            ),
        )
        .to_compile_error();
    }

    // Generate Rust watch methods
    let watch_methods: Vec<TokenStream> = watches
        .iter()
        .map(|w| {
            let method_ident = syn::Ident::new(
                &format!("watch_{}", w.field_name),
                proc_macro2::Span::call_site(),
            );
            let core_method =
                syn::Ident::new(w.field_name.as_str(), proc_macro2::Span::call_site());
            let js_name = &w.js_name;

            quote! {
                #[::wasm_bindgen::prelude::wasm_bindgen(js_name = #js_name)]
                pub fn #method_ident(&self, f: ::js_sys::Function) -> u32 {
                    let h = self.core.#core_method();
                    self.bridge.watch_json(move || h.get(), f)
                }
            }
        })
        .collect();

    // Generate and write the TS binding file
    let ts_content = generate_ts(&struct_name_str, &watches);
    let file_name = kebab_case(&struct_name_str);
    let write_result = write_ts_file(&file_name, &ts_content);
    // Emit a compile_error if the file write fails, so it's visible
    let write_err = match write_result {
        Ok(_) => TokenStream::new(),
        Err(e) => syn::Error::new_spanned(
            struct_name,
            format!("plum: failed to write TS binding file: {}", e),
        )
        .to_compile_error(),
    };

    // Factory function: create_{snake}( ) -> Wasm{Name}, js_name = create{Pascal}
    let factory_fn_name = syn::Ident::new(
        &format!("create_{}", pascal_to_snake(&struct_name_str)),
        proc_macro2::Span::call_site(),
    );
    let factory_js_name = format!("create{}", struct_name_str);

    // Generate a test that writes types.ts via ts-rs export_to_string
    let types_test = {
        let used_types: Vec<&str> = {
            let mut seen: Vec<&str> = Vec::new();
            for w in &watches {
                let base = extract_base_type(&w.ts_type);
                if !is_ts_primitive(base) && !seen.contains(&base) {
                    seen.push(base);
                }
            }
            seen
        };
        if used_types.is_empty() {
            TokenStream::new()
        } else {
            let type_exports: Vec<TokenStream> = used_types
                .iter()
                .map(|ty| {
                    let ty_ident = syn::Ident::new(ty, proc_macro2::Span::call_site());
                    quote! {
                        out.push_str(&<#ty_ident as ::ts_rs::TS>::export_to_string(&cfg).unwrap());
                        out.push_str("\n\n");
                    }
                })
                .collect();
            quote! {
                #[cfg(test)]
                #[test]
                fn _plum_gen_types() {
                    let cfg = ::ts_rs::Config::default();
                    let mut out = String::from(
                        "// AUTO-GENERATED by ts-rs via PlumModel — do not edit.\n",
                    );
                    #(#type_exports)*
                    let dir = format!("{}/plum_gen", env!("CARGO_MANIFEST_DIR"));
                    let path = format!("{}/types.ts", dir);
                    let existing = std::fs::read_to_string(&path).unwrap_or_default();
                    if existing != out {
                        std::fs::create_dir_all(&dir).unwrap();
                        std::fs::write(&path, &out).unwrap();
                    }
                }
            }
        }
    };

    quote! {
        #[cfg(feature = "plum")]
        #[::wasm_bindgen::prelude::wasm_bindgen]
        pub struct #wasm_name_ident {
            core: #struct_name,
            bridge: ::plum_wasm::bridge::Bridge,
        }

        #[cfg(feature = "plum")]
        impl #wasm_name_ident {
            /// Creates the wasm wrapper around an already-constructed core model.
            pub fn new_with(core: #struct_name) -> Self {
                Self {
                    core,
                    bridge: ::plum_wasm::bridge::Bridge::new(),
                }
            }
        }

        #[cfg(feature = "plum")]
        #[::wasm_bindgen::prelude::wasm_bindgen]
        impl #wasm_name_ident {
            #(#watch_methods)*

            pub fn unwatch(&self, id: u32) {
                self.bridge.unwatch(id);
            }

            pub fn dispose(&self) {
                self.core.dispose();
                self.bridge.unwatch_all();
            }
        }

        #write_err

        #types_test

        #[cfg(feature = "plum")]
        #[::wasm_bindgen::prelude::wasm_bindgen(js_name = #factory_js_name)]
        pub fn #factory_fn_name() -> #wasm_name_ident {
            ::plum_wasm::runtime::init();
            #wasm_name_ident::new_with(#struct_name::new())
        }
    }
}

/// Parse a field-level `#[plum(watch)]` or `#[plum(watch, js = "...")]` attribute.
/// Returns `Some(Option<String>)` — the outer Some means "is a watch",
/// the inner Option is the optional JS name override.
fn parse_watch_attr(attr: &syn::Attribute) -> Option<Option<String>> {
    let list = match &attr.meta {
        syn::Meta::List(l) => l,
        _ => return None,
    };

    let mut is_watch = false;
    let mut js_override: Option<String> = None;

    let err = list.parse_nested_meta(|nested| {
        if nested.path.is_ident("watch") {
            is_watch = true;
        } else if nested.path.is_ident("js") {
            let value = nested.value()?;
            let s: syn::LitStr = value.parse()?;
            js_override = Some(s.value());
        }
        Ok(())
    });

    if err.is_err() {
        return None;
    }
    if is_watch {
        Some(js_override)
    } else {
        None
    }
}

/// Generate the TypeScript binding file content.
fn generate_ts(struct_name: &str, watches: &[WatchInfo]) -> String {
    let pascal = pascal_case(struct_name);
    let mut out = String::new();

    out.push_str("// AUTO-GENERATED by #[derive(PlumModel)] — do not edit.\n");
    out.push_str("import { atom, type ReadableAtom } from \"nanostores\";\n");

    // Import types from same-directory types.ts (generated by ts-rs)
    {
        let used_types: Vec<&str> = {
            let mut seen: Vec<&str> = Vec::new();
            for w in watches {
                let base = extract_base_type(&w.ts_type);
                if !is_ts_primitive(base) && !seen.contains(&base) {
                    seen.push(base);
                }
            }
            seen
        };
        if !used_types.is_empty() {
            let types_list = used_types.join(", ");
            out.push_str(&format!(
                "import type {{ {} }} from \"./types\";\n",
                types_list
            ));
        }
    }

    out.push('\n');

    // Stores interface
    out.push_str(&format!("export interface {}Stores {{\n", pascal));
    for w in watches {
        let ts_var = &w.store_key;
        out.push_str(&format!("  {}: ReadableAtom<{}>;\n", ts_var, w.ts_type));
    }
    out.push_str("}\n\n");

    // The bridge plumbing on the wasm class: what bind needs, and what is
    // hidden from the `actions` type handed to consumers.
    out.push_str(&format!("interface {}WatchMethods {{\n", pascal));
    for w in watches {
        out.push_str(&format!(
            "  {}(cb: (v: {}) => void): number;\n",
            w.js_name, w.ts_type
        ));
    }
    out.push_str("  unwatch(id: number): void;\n");
    out.push_str("  dispose(): void;\n");
    out.push_str("}\n\n");

    // Rust pushes the current value synchronously inside watch*(), so a
    // store never holds `undefined` by the time anyone can read it.
    out.push_str(
        "function readable<T>(watch: (cb: (v: T) => void) => number): ReadableAtom<T> {\n",
    );
    out.push_str("  const store = atom<T>(undefined as T);\n");
    out.push_str("  watch((v) => store.set(v));\n");
    out.push_str("  return store;\n");
    out.push_str("}\n\n");

    // Bind function (generic so actions retain their concrete signatures)
    out.push_str(&format!(
        "export function bind{}<M extends {}WatchMethods>(model: M): {{\n  stores: {}Stores;\n  actions: Omit<M, keyof {}WatchMethods | \"free\">;\n  dispose: () => void;\n}} {{\n",
        pascal, pascal, pascal, pascal
    ));
    out.push_str("  return {\n");
    out.push_str("    stores: {\n");
    for w in watches {
        out.push_str(&format!(
            "      {}: readable((cb) => model.{}(cb)),\n",
            w.store_key, w.js_name
        ));
    }
    out.push_str("    },\n");
    out.push_str("    actions: model,\n");
    // The Rust-side dispose() also stops every watch.
    out.push_str("    dispose: () => model.dispose(),\n");
    out.push_str("  };\n");
    out.push_str("}\n");

    out
}

/// Extract the base type name from a TS type string (strip [] and | null).
fn extract_base_type(ts_type: &str) -> &str {
    let s = ts_type.trim();
    // Strip trailing []
    let s = s.strip_suffix("[]").unwrap_or(s);
    // Strip " | null"
    let s = s.strip_suffix(" | null").unwrap_or(s);
    s.trim()
}

/// Returns true if the type is a TS built-in primitive (should not be imported).
fn is_ts_primitive(ty: &str) -> bool {
    matches!(
        ty,
        "number" | "boolean" | "string" | "void" | "any" | "unknown" | "null" | "undefined"
    )
}

/// Write the TS file to plum_gen/ only if content changed.
fn write_ts_file(file_name: &str, content: &str) -> Result<(), String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|e| format!("CARGO_MANIFEST_DIR not set: {}", e))?;
    let dir = format!("{}/plum_gen", manifest_dir);
    let path = format!("{}/{}.ts", dir, file_name);

    // Read existing content
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing == content {
        return Ok(());
    }

    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create {}: {}", dir, e))?;
    std::fs::write(&path, content).map_err(|e| format!("failed to write {}: {}", path, e))
}
