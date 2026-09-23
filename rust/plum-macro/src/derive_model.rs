//! `#[derive(PlumModel)]`: the wasm-bindgen wrapper around a model struct, and
//! the test that writes the model's TypeScript binding.
//!
//! The macro only deals in names. Everything about types is left to the
//! compiler: a watched field has to implement `Get`, its value has to be
//! `Serialize` and `TS`, and ts-rs names it, whatever it is.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Ident, Type};

use crate::naming::{
    kebab_case, lower_first, pascal_case, snake_case, snake_to_camel, wasm_class_name,
};

struct Watch<'a> {
    field: &'a Ident,
    ty: &'a Type,
    /// The watch method on the wasm class, e.g. `watchTodos`.
    js_name: String,
    /// The key in the generated stores object, e.g. `todos`.
    store_key: String,
    setter: Setter,
    /// Doc comment from the field (for manifest generation).
    doc: String,
}

/// How JS writes to a store, if it can.
enum Setter {
    None,
    /// `#[plum(watch, set)]`: the field itself is set.
    Direct,
    /// `#[plum(watch, set = "method")]`: that method of the model is called.
    Method(String),
}

#[derive(Default)]
struct FieldOptions {
    watch: bool,
    js: Option<String>,
    set: Option<Option<String>>,
}

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let model = &input.ident;
    let model_name = model.to_string();
    let wasm = Ident::new(&wasm_class_name(&model_name), Span::call_site());

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "plum: a model cannot have generic parameters, because wasm-bindgen cannot export one",
        ));
    }
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    model,
                    "plum: PlumModel needs a struct with named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                model,
                "plum: PlumModel can only be derived on a struct",
            ))
        }
    };

    let mut watches = Vec::new();
    for field in fields {
        let name = field.ident.as_ref().expect("named field");
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("plum")) {
            let options = parse_field(attr)?;
            if !options.watch {
                continue;
            }
            let base = options.js.unwrap_or_else(|| name.to_string());
            let doc = field.attrs.iter()
                .filter(|a| a.path().is_ident("doc"))
                .filter_map(|a| {
                    if let syn::Meta::NameValue(nv) = &a.meta {
                        if let syn::Expr::Lit(expr) = &nv.value {
                            if let syn::Lit::Str(lit) = &expr.lit {
                                return Some(lit.value().trim_start().to_string());
                            }
                        }
                    }
                    None
                })
                .collect::<Vec<_>>()
                .join("\n");
            watches.push(Watch {
                field: name,
                ty: &field.ty,
                js_name: format!("watch{}", pascal_case(&base)),
                store_key: lower_first(&snake_to_camel(&base)),
                setter: match options.set {
                    None => Setter::None,
                    Some(None) => Setter::Direct,
                    Some(Some(method)) => Setter::Method(method),
                },
                doc,
            });
        }
    }

    let watch_methods = watches.iter().map(|w| {
        let method = format_ident!("watch_{}", w.field);
        let (field, js_name) = (w.field, &w.js_name);
        // Spanned to the field's type, so that a missing `Get` or `Serialize`
        // is reported on the field and not on the derive.
        let body = quote_spanned! {w.ty.span()=>
            let source = ::core::clone::Clone::clone(&self.core.#field);
            self.bridge
                .watch_json(move || ::leptos::reactive::traits::Get::get(&source), f)
        };
        quote! {
            #[::wasm_bindgen::prelude::wasm_bindgen(js_name = #js_name)]
            pub fn #method(&self, f: ::plum_wasm::__rt::js_sys::Function) -> u32 {
                #body
            }
        }
    });

    // `#[plum(watch, set)]`: JS sets the field itself. The name is one no
    // action can have.
    let set_methods = watches
        .iter()
        .filter(|w| matches!(w.setter, Setter::Direct))
        .map(|w| {
            let method = format_ident!("__plum_set_{}", w.field);
            let js_name = direct_setter_name(w);
            let (field, ty, key) = (w.field, w.ty, &w.store_key);
            let body = quote_spanned! {ty.span()=>
                let value: <#ty as ::leptos::reactive::traits::Set>::Value =
                    ::plum_wasm::__rt::arg(#key, "value", value)?;
                ::leptos::reactive::traits::Set::set(&self.core.#field, value);
                self.bridge.flush();
                ::core::result::Result::Ok(())
            };
            quote! {
                #[::wasm_bindgen::prelude::wasm_bindgen(js_name = #js_name)]
                pub fn #method(
                    &self,
                    value: ::wasm_bindgen::JsValue,
                ) -> ::core::result::Result<(), ::wasm_bindgen::JsValue> {
                    #body
                }
            }
        });

    let export = export_test(model, &model_name, &watches);
    let watched = watches.iter().map(|w| w.field);

    Ok(quote! {
        impl #model {
            // The watched fields are read by the code below, which is compiled
            // out without the `plum` feature. This read keeps the compiler
            // from calling them dead in that build.
            #[doc(hidden)]
            pub fn __plum_watched(&self) {
                let _ = (#(&self.#watched,)*);
            }
        }

        #[cfg(feature = "plum")]
        #[::wasm_bindgen::prelude::wasm_bindgen]
        pub struct #wasm {
            core: ::std::rc::Rc<#model>,
            bridge: ::plum_wasm::Bridge,
        }

        #[cfg(feature = "plum")]
        impl #wasm {
            /// Wraps a model that was constructed by hand. Call
            /// `plum_wasm::runtime::init()` before constructing it.
            pub fn new_with(core: #model) -> Self {
                Self {
                    core: ::std::rc::Rc::new(core),
                    bridge: ::plum_wasm::Bridge::new(),
                }
            }

            #[doc(hidden)]
            pub fn __plum(&self) -> (&::std::rc::Rc<#model>, &::plum_wasm::Bridge) {
                (&self.core, &self.bridge)
            }
        }

        #[cfg(feature = "plum")]
        #[::wasm_bindgen::prelude::wasm_bindgen]
        impl #wasm {
            #(#watch_methods)*

            #(#set_methods)*

            pub fn unwatch(&self, id: u32) {
                self.bridge.unwatch(id);
            }

            pub fn dispose(&self) {
                // An inherent `dispose(&self)` on the model takes precedence
                // over this trait; a model without one gets the empty default.
                #[allow(dead_code)]
                trait NoDispose {
                    fn dispose(&self) {}
                }
                impl NoDispose for #model {}
                self.core.dispose();
                self.bridge.unwatch_all();
            }
        }

        #export
    })
}

/// `#[plum(watch)]`, with `js = "Name"`, `set` or `set = "method"`.
fn parse_field(attr: &syn::Attribute) -> syn::Result<FieldOptions> {
    let mut options = FieldOptions::default();
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("watch") {
            options.watch = true;
            Ok(())
        } else if meta.path.is_ident("js") {
            options.js = Some(meta.value()?.parse::<syn::LitStr>()?.value());
            Ok(())
        } else if meta.path.is_ident("set") {
            options.set = Some(match meta.value() {
                Ok(value) => Some(value.parse::<syn::LitStr>()?.value()),
                Err(_) => None,
            });
            Ok(())
        } else {
            Err(meta.error("plum: expected `watch`, `set`, `set = \"method\"` or `js = \"...\"`"))
        }
    })?;
    Ok(options)
}

/// "plumSetInputText": the wasm method behind `#[plum(watch, set)]`.
fn direct_setter_name(watch: &Watch) -> String {
    format!("plumSet{}", &watch.js_name["watch".len()..])
}

/// The test that writes `plum_gen/<model>.ts`. It runs with real type
/// information, which the macro itself never has, so ts-rs can name every
/// watched value and find every struct and enum nested inside one.
fn export_test(model: &Ident, model_name: &str, watches: &[Watch]) -> TokenStream {
    let test = format_ident!("__plum_export_{}", snake_case(model_name));
    let file = format!("{}.ts", kebab_case(model_name));
    let pascal = pascal_case(model_name);
    let keys = watches.iter().map(|w| &w.store_key);
    let js_names = watches.iter().map(|w| &w.js_name);
    let docs = watches.iter().map(|w| &w.doc);
    let values = watches.iter().map(|w| {
        let ty = w.ty;
        quote_spanned! {ty.span()=> <#ty as ::leptos::reactive::traits::Get>::Value }
    });
    let json_file = format!("{}.json", kebab_case(model_name));
    // The method JS calls to write the store, and whether it is an action
    // of the model (which then stops being listed as one).
    let writers = watches.iter().map(|w| match &w.setter {
        Setter::None => quote!(None),
        Setter::Direct => {
            let name = direct_setter_name(w);
            quote!(Some((#name, false)))
        }
        Setter::Method(method) => {
            let name = snake_to_camel(method);
            quote!(Some((#name, true)))
        }
    });

    quote! {
        #[cfg(test)]
        #[test]
        fn #test() {
            use ::std::fmt::Write as _;
            use ::ts_rs::{Config, TypeVisitor, TS};

            /// Collects the declaration of every named type it is shown,
            /// and of every named type inside those.
            struct Named<'a>(&'a Config, &'a mut ::std::collections::BTreeMap<String, String>);
            impl TypeVisitor for Named<'_> {
                fn visit<T: TS + 'static + ?Sized>(&mut self) {
                    if T::output_path().is_some() {
                        let name = T::ident(self.0);
                        if !self.1.contains_key(&name) {
                            self.1.insert(name, T::decl(self.0));
                            T::visit_dependencies(self);
                        }
                    }
                    T::visit_generics(self);
                }
            }

            // What `#[plum_actions]` contributes: the actions as (name,
            // interface member, implementation), then the interface members
            // and implementations of its stores, then what those stores need
            // from the wasm class. Its inherent `__plum_actions` takes
            // precedence over this trait; a model without one gets nothing.
            type Actions = (
                Vec<(&'static str, String, String, &'static str)>,
                Vec<(&'static str, Vec<(String, String)>, String, &'static str)>,
                Vec<(&'static str, &'static str, String, Vec<(String, String)>, &'static str)>,
                String, String, String,
            );
            #[allow(dead_code)]
            trait NoActions {
                fn __plum_actions<V: TypeVisitor>(_: &Config, _: &mut V) -> Actions {
                    Default::default()
                }
            }
            impl NoActions for #model {}

            // 64-bit integers cross as JS numbers, not bigints.
            let cfg = Config::new().with_large_int("number");
            let mut named = ::std::collections::BTreeMap::new();
            let mut visitor = Named(&cfg, &mut named);

            let stores: Vec<(&str, &str, String, Option<(&str, bool)>, &str)> = vec![#((
                #keys,
                #js_names,
                {
                    visitor.visit::<#values>();
                    <#values as TS>::name(&cfg)
                },
                #writers,
                #docs,
            )),*];
            let (actions, action_entries, store_entries, store_sigs, store_fns, mut wasm_sigs): Actions =
                <#model>::__plum_actions(&cfg, &mut visitor);

            // A method that writes a store is reached through `store.set`.
            let is_setter = |name: &str| {
                stores.iter().any(|(_, _, _, writer, _)| *writer == Some((name, true)))
            };
            let (mut action_sigs, mut action_fns) = (String::new(), String::new());
            for (name, sig, implementation, _) in &actions {
                if is_setter(name) {
                    wasm_sigs.push_str(sig);
                } else {
                    action_sigs.push_str(sig);
                    action_fns.push_str(implementation);
                }
            }

            let mut out = String::from(concat!(
                "// AUTO-GENERATED by plum from `", #model_name, "` — do not edit.\n",
                "import { atom, mapCreator, onMount, type ReadableAtom, type WritableAtom } from \"nanostores\";\n\n",
            ));
            for decl in named.values() {
                writeln!(out, "export {decl}\n").unwrap();
            }
            writeln!(out, "export interface {}Stores {{", #pascal).unwrap();
            for (key, _, ty, writer, _) in &stores {
                let kind = if writer.is_some() { "WritableAtom" } else { "ReadableAtom" };
                writeln!(out, "  {key}: {kind}<{ty}>;").unwrap();
            }
            writeln!(out, "{store_sigs}}}\n\nexport interface {}Actions {{\n{action_sigs}}}\n", #pascal).unwrap();
            writeln!(out, "/** What bind{0} needs from the wasm class. */", #pascal).unwrap();
            writeln!(out, "interface {0}Wasm extends {0}Actions {{", #pascal).unwrap();
            for (_, watch, ty, writer, _) in &stores {
                writeln!(out, "  {watch}(cb: (v: {ty}) => void): number;").unwrap();
                if let Some((setter, false)) = writer {
                    writeln!(out, "  {setter}(value: {ty}): void;").unwrap();
                }
            }
            out.push_str(&wasm_sigs);
            out.push_str("  unwatch(id: number): void;\n  dispose(): void;\n}\n\n");

            let mut body = String::new();
            for (key, watch, ty, writer, _) in &stores {
                match writer {
                    None => writeln!(
                        body,
                        "      {key}: readable<{ty}>((cb) => model.{watch}(cb), unwatch),"
                    ),
                    Some((setter, _)) => writeln!(
                        body,
                        "      {key}: writable<{ty}>((cb) => model.{watch}(cb), unwatch, (v) => model.{setter}(v)),"
                    ),
                }
                .unwrap();
            }
            body.push_str(&store_fns);

            out.push_str(concat!(
                "// A store is subscribed in Rust while it has listeners. Rust calls back\n",
                "// with the current value inside watch*(), so a store that is listened to\n",
                "// or read never holds `undefined`.\n",
                "function readable<T>(\n",
                "  watch: (cb: (v: T) => void) => number,\n",
                "  unwatch: (id: number) => void,\n",
                "): ReadableAtom<T> {\n",
                "  const store = atom<T>(undefined as T);\n",
                "  onMount(store, () => {\n",
                "    const id = watch((v) => store.set(v));\n",
                "    return () => unwatch(id);\n",
                "  });\n",
                "  return store;\n",
                "}\n\n",
            ));
            if body.contains("writable<") {
                out.push_str(concat!(
                    "// `set` on a writable store goes to Rust. The new value comes back\n",
                    "// through the subscription, before `set` returns.\n",
                    "function writable<T>(\n",
                    "  watch: (cb: (v: T) => void) => number,\n",
                    "  unwatch: (id: number) => void,\n",
                    "  write: (v: T) => void,\n",
                    "): WritableAtom<T> {\n",
                    "  const store = atom<T>(undefined as T);\n",
                    "  const receive = store.set;\n",
                    "  store.set = write;\n",
                    "  onMount(store, () => {\n",
                    "    const id = watch(receive);\n",
                    "    return () => unwatch(id);\n",
                    "  });\n",
                    "  return store;\n",
                    "}\n\n",
                ));
            }
            if body.contains("_paramStores(") {
                out.push_str(concat!(
                    "// Deterministic, type-tagged canonical encoding for parameterized store keys.\n",
                    "// Logically-equal arguments always yield the same key, independent of\n",
                    "// object key insertion order.\n",
                    "function canonical(v: unknown): string {\n",
                    "  if (v === null) return \"n\";\n",
                    "  if (v === undefined) return \"u\";\n",
                    "  switch (typeof v) {\n",
                    "    case \"boolean\": return v ? \"T\" : \"F\";\n",
                    "    case \"number\":\n",
                    "      if (Number.isNaN(v)) return \"NaN\";\n",
                    "      if (!Number.isFinite(v)) return v > 0 ? \"Inf\" : \"-Inf\";\n",
                    "      return \"d\" + v;\n",
                    "    case \"string\": return \"s\" + v;\n",
                    "    case \"bigint\": return \"g\" + v;\n",
                    "    case \"object\": {\n",
                    "      if (Array.isArray(v)) return \"a[\" + v.map(canonical).join(\",\") + \"]\";\n",
                    "      const o = v as Record<string, unknown>;\n",
                    "      const keys = Object.keys(o).sort();\n",
                    "      return \"o{\" + keys.map((k) => canonical(k) + \"=\" + canonical(o[k])).join(\",\") + \"}\";\n",
                    "    }\n",
                    "    default: return String(v);\n",
                    "  }\n",
                    "}\n",
                    "function sharedKey(args: unknown[]): string {\n",
                    "  return args.map(canonical).join(\"\\x1f\");\n",
                    "}\n",
                    "// Parameterized stores backed by nanostores mapCreator: the cache entry\n",
                    "// and its wasm watch are torn down when the last listener unmounts.\n",
                    "const _paramStores = mapCreator<any, any[]>((store, _id, watchFn, unwatchFn, writeFn) => {\n",
                    "  const _set = store.set.bind(store);\n",
                    "  const watchId = watchFn((v: any) => _set(v));\n",
                    "  if (writeFn) store.set = (v: any) => { writeFn(v); _set(v); };\n",
                    "  return () => unwatchFn(watchId);\n",
                    "});\n\n",
                ));
            }
            write!(
                out,
                "export function bind{0}(model: {0}Wasm): {{\n  stores: {0}Stores;\n  actions: {0}Actions;\n  dispose: () => void;\n}} {{\n  const unwatch = (id: number) => model.unwatch(id);\n  return {{\n    stores: {{\n{body}    }},\n    actions: {{\n{action_fns}    }},\n    dispose: () => model.dispose(),\n  }};\n}}\n",
                #pascal
            )
            .unwrap();

            let dir = ::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plum_gen");
            ::std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(#file);
            if ::std::fs::read_to_string(&path).ok().as_deref() != Some(out.as_str()) {
                ::std::fs::write(&path, out).unwrap();
            }

            // --- JSON manifest for doc generation ---
            fn jesc(s: &str) -> String {
                let mut o = String::with_capacity(s.len());
                for c in s.chars() {
                    match c {
                        '"' => o.push_str("\\\""),
                        '\\' => o.push_str("\\\\"),
                        '\n' => o.push_str("\\n"),
                        '\r' => o.push_str("\\r"),
                        '\t' => o.push_str("\\t"),
                        c if (c as u32) < 0x20 => {
                            let _ = ::std::fmt::Write::write_fmt(&mut o, format_args!("\\u{:04x}", c as u32));
                        }
                        _ => o.push(c),
                    }
                }
                o
            }

            let mut store_lines: Vec<String> = Vec::new();
            for (key, _, ty, writer, doc) in &stores {
                let kind = if writer.is_some() { "writable" } else { "readable" };
                store_lines.push(format!(
                    "    {{\"name\":\"{}\",\"kind\":\"{}\",\"value\":\"{}\",\"args\":[],\"doc\":\"{}\"}}",
                    jesc(key), kind, jesc(ty), jesc(doc)
                ));
            }
            for (name, kind, value, args, doc) in &store_entries {
                let args_json: Vec<String> = args.iter()
                    .map(|(n, t)| format!("{{\"name\":\"{}\",\"type\":\"{}\"}}", jesc(n), jesc(t)))
                    .collect();
                store_lines.push(format!(
                    "    {{\"name\":\"{}\",\"kind\":\"{}\",\"value\":\"{}\",\"args\":[{}],\"doc\":\"{}\"}}",
                    jesc(name), kind, jesc(value), args_json.join(","), jesc(doc)
                ));
            }

            let mut action_lines: Vec<String> = Vec::new();
            for (name, params, ret, doc) in &action_entries {
                let params_json: Vec<String> = params.iter()
                    .map(|(n, t)| format!("{{\"name\":\"{}\",\"type\":\"{}\"}}", jesc(n), jesc(t)))
                    .collect();
                action_lines.push(format!(
                    "    {{\"name\":\"{}\",\"params\":[{}],\"returns\":\"{}\",\"doc\":\"{}\"}}",
                    jesc(name), params_json.join(","), jesc(ret), jesc(doc)
                ));
            }

            let mut type_lines: Vec<String> = Vec::new();
            for (name, decl) in &named {
                type_lines.push(format!(
                    "    {{\"name\":\"{}\",\"decl\":\"{}\"}}",
                    jesc(name), jesc(decl)
                ));
            }

            let mut json = String::new();
            json.push_str("{\n");
            json.push_str(&format!("  \"model\":\"{}\",\n", jesc(#model_name)));
            json.push_str(&format!("  \"stores\":[\n{}\n  ],\n", store_lines.join(",\n")));
            json.push_str(&format!("  \"actions\":[\n{}\n  ],\n", action_lines.join(",\n")));
            json.push_str(&format!("  \"types\":[\n{}\n  ]\n", type_lines.join(",\n")));
            json.push_str("}\n");

            let json_path = dir.join(#json_file);
            if ::std::fs::read_to_string(&json_path).ok().as_deref() != Some(json.as_str()) {
                ::std::fs::write(&json_path, &json).unwrap();
            }
        }
    }
}
