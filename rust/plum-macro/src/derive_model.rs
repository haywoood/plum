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
            if let Some(js) = parse_watch(attr)? {
                let base = js.unwrap_or_else(|| name.to_string());
                watches.push(Watch {
                    field: name,
                    ty: &field.ty,
                    js_name: format!("watch{}", pascal_case(&base)),
                    store_key: lower_first(&snake_to_camel(&base)),
                });
            }
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

/// `#[plum(watch)]` or `#[plum(watch, js = "Name")]`. Returns `None` for a
/// `#[plum(...)]` attribute that is not a watch.
fn parse_watch(attr: &syn::Attribute) -> syn::Result<Option<Option<String>>> {
    let mut watch = false;
    let mut js = None;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("watch") {
            watch = true;
            Ok(())
        } else if meta.path.is_ident("js") {
            js = Some(meta.value()?.parse::<syn::LitStr>()?.value());
            Ok(())
        } else {
            Err(meta.error("plum: expected `watch` or `js = \"...\"`"))
        }
    })?;
    Ok(watch.then_some(js))
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
    let values = watches.iter().map(|w| {
        let ty = w.ty;
        quote_spanned! {ty.span()=> <#ty as ::leptos::reactive::traits::Get>::Value }
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

            // `#[plum_actions]` defines an inherent `__plum_actions`, which
            // takes precedence over this; a model without actions gets none.
            #[allow(dead_code)]
            trait NoActions {
                fn __plum_actions<V: TypeVisitor>(_: &Config, _: &mut V) -> [String; 5] {
                    Default::default()
                }
            }
            impl NoActions for #model {}

            // 64-bit integers cross as JS numbers, not bigints.
            let cfg = Config::new().with_large_int("number");
            let mut named = ::std::collections::BTreeMap::new();
            let mut visitor = Named(&cfg, &mut named);

            let stores: Vec<(&str, &str, String)> = vec![#((
                #keys,
                #js_names,
                {
                    visitor.visit::<#values>();
                    <#values as TS>::name(&cfg)
                },
            )),*];
            let [action_sigs, action_fns, store_sigs, store_fns, wasm_sigs] =
                <#model>::__plum_actions(&cfg, &mut visitor);

            let mut out = String::from(concat!(
                "// AUTO-GENERATED by plum from `", #model_name, "` — do not edit.\n",
                "import { atom, onMount, type ReadableAtom } from \"nanostores\";\n\n",
            ));
            for decl in named.values() {
                writeln!(out, "export {decl}\n").unwrap();
            }
            writeln!(out, "export interface {}Stores {{", #pascal).unwrap();
            for (key, _, ty) in &stores {
                writeln!(out, "  {key}: ReadableAtom<{ty}>;").unwrap();
            }
            writeln!(out, "{store_sigs}}}\n\nexport interface {}Actions {{\n{action_sigs}}}\n", #pascal).unwrap();
            writeln!(out, "/** What bind{0} needs from the wasm class. */", #pascal).unwrap();
            writeln!(out, "interface {0}Wasm extends {0}Actions {{", #pascal).unwrap();
            for (_, watch, ty) in &stores {
                writeln!(out, "  {watch}(cb: (v: {ty}) => void): number;").unwrap();
            }
            out.push_str(&wasm_sigs);
            out.push_str(concat!(
                "  unwatch(id: number): void;\n",
                "  dispose(): void;\n}\n\n",
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
            if store_fns.contains("family<") {
                out.push_str(concat!(
                    "// Stores that take arguments: the same arguments give the same store.\n",
                    "function family<A extends unknown[], T>(\n",
                    "  watch: (...args: [...A, (v: T) => void]) => number,\n",
                    "  unwatch: (id: number) => void,\n",
                    "): (...args: A) => ReadableAtom<T> {\n",
                    "  const stores = new Map<string, ReadableAtom<T>>();\n",
                    "  return (...args) => {\n",
                    "    const key = JSON.stringify(args);\n",
                    "    let store = stores.get(key);\n",
                    "    if (!store) {\n",
                    "      store = readable((cb) => watch(...args, cb), unwatch);\n",
                    "      stores.set(key, store);\n",
                    "    }\n",
                    "    return store;\n",
                    "  };\n",
                    "}\n\n",
                ));
            }
            writeln!(
                out,
                "export function bind{0}(model: {0}Wasm): {{\n  stores: {0}Stores;\n  actions: {0}Actions;\n  dispose: () => void;\n}} {{\n  const unwatch = (id: number) => model.unwatch(id);\n  return {{\n    stores: {{",
                #pascal
            )
            .unwrap();
            for (key, watch, _) in &stores {
                writeln!(out, "      {key}: readable((cb) => model.{watch}(cb), unwatch),").unwrap();
            }
            write!(
                out,
                "{store_fns}    }},\n    actions: {{\n{action_fns}    }},\n    dispose: () => model.dispose(),\n  }};\n}}\n"
            )
            .unwrap();

            let dir = ::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plum_gen");
            let path = dir.join(#file);
            if ::std::fs::read_to_string(&path).ok().as_deref() != Some(out.as_str()) {
                ::std::fs::create_dir_all(&dir).unwrap();
                ::std::fs::write(&path, out).unwrap();
            }
        }
    }
}
