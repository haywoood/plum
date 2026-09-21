//! `#[plum_actions]`: exports the methods of a model's `impl` block.
//!
//! The block is passed through unchanged. Next to it the macro generates
//!
//! - an action on the wasm class for every `pub fn` that takes `&self`,
//! - a store for every such method marked `#[plum(watch)]`: it returns a
//!   reactive value, and if it takes arguments the store does too,
//! - a `create{Model}` factory from `pub fn new(..)`, with its parameters,
//! - the TypeScript for all of those, for the generated binding.
//!
//! Every argument arrives as a `JsValue` and is deserialized into the type
//! the method declares, so the macro never has to recognise a type by name.
//!
//! Actions are synchronous. Async work belongs in a Leptos `Action` that a
//! plain method dispatches; its `pending()` and `value()` are watchable.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ImplItem, ImplItemFn, ItemImpl, Pat, ReturnType, Type};

use crate::naming::{snake_case, snake_to_camel, wasm_class_name};

/// Reactive handles. A method that returns one and is not marked
/// `#[plum(watch)]` is an accessor for Rust callers, not an action.
/// `#[plum(skip)]` covers anything not listed.
const HANDLES: &[&str] = &[
    "RwSignal",
    "ReadSignal",
    "WriteSignal",
    "Signal",
    "Memo",
    "ArcRwSignal",
    "ArcReadSignal",
    "ArcWriteSignal",
    "ArcSignal",
    "ArcMemo",
    "Action",
    "ArcAction",
    "Resource",
    "LocalResource",
    "StoredValue",
    "Trigger",
];

struct Param {
    name: Ident,
    /// `text` in `fn f(&self, text: &str)` is deserialized as a `String` and
    /// passed as `&text`; `owned` is that `String`.
    owned: TokenStream,
    /// The declared type without an outer `&`, which is what ts-rs names.
    ts: Type,
    by_ref: bool,
}

#[derive(Default)]
struct Options {
    skip: bool,
    watch: bool,
    js: Option<String>,
    /// `set = "method"` on a watched method: the method that writes it.
    set: Option<syn::LitStr>,
}

pub fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let mut item: ItemImpl = syn::parse2(input)?;
    if let Some((_, path, _)) = &item.trait_ {
        return Err(syn::Error::new_spanned(
            path,
            "plum: #[plum_actions] goes on the model's inherent impl block",
        ));
    }
    let model = match &*item.self_ty {
        Type::Path(tp) if tp.qself.is_none() => tp.path.segments.last().map(|s| s.ident.clone()),
        _ => None,
    }
    .ok_or_else(|| syn::Error::new_spanned(&item.self_ty, "plum: cannot name this type"))?;
    let wasm = Ident::new(&wasm_class_name(&model.to_string()), Span::call_site());

    // First the options and JS names of everything, so that a store can
    // look up the method that writes it.
    let mut members = Vec::new();
    for member in &mut item.items {
        let ImplItem::Fn(func) = member else { continue };
        let options = take_options(func)?;
        let js = options
            .js
            .clone()
            .unwrap_or_else(|| snake_to_camel(&func.sig.ident.to_string()));
        members.push((func.clone(), options, js));
    }
    let js_name_of = |method: &syn::LitStr| {
        members
            .iter()
            .find(|(func, ..)| func.sig.ident == method.value())
            .map(|(.., js)| js.clone())
            .ok_or_else(|| syn::Error::new_spanned(method, "plum: no such method in this block"))
    };
    let mut setters = Vec::new();
    for (_, options, _) in &members {
        if let Some(method) = &options.set {
            setters.push(js_name_of(method)?);
        }
    }

    let mut actions = Vec::new();
    let mut signatures = Vec::new();
    let mut factory = TokenStream::new();
    for (func, options, js) in &members {
        if options.skip || !matches!(func.vis, syn::Visibility::Public(_)) {
            continue;
        }
        let name = func.sig.ident.to_string();
        let exported = name == "new" || matches!(func.sig.inputs.first(), Some(FnArg::Receiver(_)));
        if let (true, Some(asyncness)) = (exported, &func.sig.asyncness) {
            return Err(syn::Error::new_spanned(
                asyncness,
                "plum: an `async fn` cannot be exported. Put the work in a Leptos `Action`, \
                 dispatch it from a plain method and watch its `pending()` and `value()`; \
                 or mark this method #[plum(skip)]",
            ));
        }
        match func.sig.inputs.first() {
            Some(FnArg::Receiver(r)) if r.reference.is_some() && r.mutability.is_none() => {
                if options.watch {
                    let params = params(func)?;
                    let setter = options.set.as_ref().map(&js_name_of).transpose()?;
                    actions.push(store(func, js, &params)?);
                    signatures.push(store_signature(func, js, &params, setter.as_deref()));
                    continue;
                }
                if let Some(set) = &options.set {
                    return Err(syn::Error::new_spanned(
                        set,
                        "plum: `set` belongs on a #[plum(watch)] method",
                    ));
                }
                if name == "dispose" || returns_handle(&func.sig.output) {
                    continue;
                }
                let params = params(func)?;
                actions.push(action(func, js, &params));
                // A method that writes a store is reached through `store.set`.
                signatures.push(signature(func, js, &params, setters.contains(js)));
            }
            None | Some(FnArg::Typed(_)) if name == "new" => {
                factory = self::factory(func, &model, &wasm, &params(func)?);
            }
            _ => {}
        }
    }

    Ok(quote! {
        #item

        #[cfg(feature = "plum")]
        #[::wasm_bindgen::prelude::wasm_bindgen]
        impl #wasm {
            #(#actions)*
        }

        #factory

        #[cfg(test)]
        impl #model {
            #[doc(hidden)]
            #[allow(dead_code, unused_variables)]
            pub fn __plum_actions<V: ::ts_rs::TypeVisitor>(
                cfg: &::ts_rs::Config,
                visitor: &mut V,
            ) -> (Vec<(&'static str, String, String)>, String, String, String) {
                // The actions as (name, interface member, implementation),
                // then the interface members and implementations of the
                // stores, then what the stores need from the wasm class.
                let mut actions = Vec::new();
                let (mut store_sigs, mut store_fns) = (String::new(), String::new());
                let mut wasm_sigs = String::new();
                #(#signatures)*
                (actions, store_sigs, store_fns, wasm_sigs)
            }
        }
    })
}

/// Removes `#[plum(...)]` from a method and returns what it said.
fn take_options(func: &mut ImplItemFn) -> syn::Result<Options> {
    let mut options = Options::default();
    let mut result = Ok(());
    func.attrs.retain(|attr| {
        if !attr.path().is_ident("plum") {
            return true;
        }
        let parsed =
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    options.skip = true;
                    Ok(())
                } else if meta.path.is_ident("watch") {
                    options.watch = true;
                    Ok(())
                } else if meta.path.is_ident("js") {
                    options.js = Some(meta.value()?.parse::<syn::LitStr>()?.value());
                    Ok(())
                } else if meta.path.is_ident("set") {
                    options.set = Some(meta.value().map_err(|_| {
                    meta.error("plum: name the method that writes this store: set = \"method\"")
                })?.parse()?);
                    Ok(())
                } else {
                    Err(meta.error(
                        "plum: expected `skip`, `watch`, `set = \"method\"` or `js = \"...\"`",
                    ))
                }
            });
        if let Err(e) = parsed {
            result = Err(e);
        }
        false
    });
    result.map(|()| options)
}

fn params(func: &ImplItemFn) -> syn::Result<Vec<Param>> {
    if !func.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &func.sig.generics,
            "plum: a generic method cannot be exported; mark it #[plum(skip)]",
        ));
    }
    func.sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Typed(typed) => Some(typed),
            FnArg::Receiver(_) => None,
        })
        .map(|typed| {
            let Pat::Ident(pat) = &*typed.pat else {
                return Err(syn::Error::new_spanned(
                    &typed.pat,
                    "plum: an exported parameter needs a plain name",
                ));
            };
            let name = pat.ident.clone();
            match &*typed.ty {
                Type::Reference(r) if r.mutability.is_some() => Err(syn::Error::new_spanned(
                    r,
                    "plum: `&mut` parameters cannot come from JS; mark the method #[plum(skip)]",
                )),
                Type::Reference(r) => {
                    let inner = &*r.elem;
                    let owned = match inner {
                        Type::Path(p) if p.path.is_ident("str") => quote!(::std::string::String),
                        Type::Slice(s) => {
                            let elem = &s.elem;
                            quote!(::std::vec::Vec<#elem>)
                        }
                        other => quote!(#other),
                    };
                    Ok(Param {
                        name,
                        owned,
                        ts: inner.clone(),
                        by_ref: true,
                    })
                }
                ty => Ok(Param {
                    name,
                    owned: quote!(#ty),
                    ts: ty.clone(),
                    by_ref: false,
                }),
            }
        })
        .collect()
}

/// `let text: String = arg("add", "text", text)?;` for every parameter.
fn conversions(js: &str, params: &[Param]) -> TokenStream {
    params
        .iter()
        .map(|p| {
            let (name, owned) = (&p.name, &p.owned);
            let label = snake_to_camel(&name.to_string());
            quote_spanned! {p.ts.span()=>
                let #name: #owned = ::plum_wasm::__rt::arg(#js, #label, #name)?;
            }
        })
        .collect()
}

fn call_args(params: &[Param]) -> Vec<TokenStream> {
    params
        .iter()
        .map(|p| {
            let name = &p.name;
            if p.by_ref {
                quote!(&#name)
            } else {
                quote!(#name)
            }
        })
        .collect()
}

fn action(func: &ImplItemFn, js: &str, params: &[Param]) -> TokenStream {
    let name = &func.sig.ident;
    let names = params.iter().map(|p| &p.name);
    let conversions = conversions(js, params);
    let args = call_args(params);
    let undefined = quote!(::core::result::Result::Ok(
        ::wasm_bindgen::JsValue::UNDEFINED
    ));

    let body = match returned(&func.sig.output) {
        Returned::Nothing => quote! {
            core.#name(#(#args),*);
            bridge.flush();
            #undefined
        },
        Returned::Value(_) => quote! {
            let out = core.#name(#(#args),*);
            bridge.flush();
            ::plum_wasm::__rt::ret(#js, &out)
        },
        Returned::Fallible(ok) => {
            let ok = match ok {
                Some(_) => quote!(::plum_wasm::__rt::ret(#js, &value)),
                None => undefined.clone(),
            };
            quote! {
                let out = core.#name(#(#args),*);
                bridge.flush();
                match out {
                    ::core::result::Result::Ok(value) => {
                        let _ = &value;
                        #ok
                    }
                    ::core::result::Result::Err(e) => {
                        ::core::result::Result::Err(::plum_wasm::__rt::err(e))
                    }
                }
            }
        }
    };

    quote! {
        #[::wasm_bindgen::prelude::wasm_bindgen(js_name = #js)]
        pub fn #name(
            &self,
            #(#names: ::wasm_bindgen::JsValue),*
        ) -> ::core::result::Result<::wasm_bindgen::JsValue, ::wasm_bindgen::JsValue> {
            let (core, bridge) = self.__plum();
            #conversions
            #body
        }
    }
}

/// A `#[plum(watch)]` method: `watch{Name}(args.., callback)` on the wasm
/// class. The method is called once per subscription, inside a scope that is
/// cleaned up on unwatch, so a memo it creates lives as long as the store has
/// listeners.
fn store(func: &ImplItemFn, js: &str, params: &[Param]) -> syn::Result<TokenStream> {
    let ReturnType::Type(_, returned) = &func.sig.output else {
        return Err(syn::Error::new_spanned(
            &func.sig,
            "plum: a #[plum(watch)] method returns the signal or memo to watch",
        ));
    };
    let name = &func.sig.ident;
    let rust = format_ident!("__plum_watch_{}", name);
    let watch = watch_name(js);
    let names = params.iter().map(|p| &p.name);
    let conversions = conversions(js, params);
    let args = call_args(params);
    let body = quote_spanned! {returned.span()=>
        let scope = bridge.scope();
        let source = scope.with(|| core.#name(#(#args),*));
        ::core::result::Result::Ok(bridge.watch_json_scoped(
            scope,
            move || ::leptos::reactive::traits::Get::get(&source),
            f,
        ))
    };
    Ok(quote! {
        #[::wasm_bindgen::prelude::wasm_bindgen(js_name = #watch)]
        pub fn #rust(
            &self,
            #(#names: ::wasm_bindgen::JsValue,)*
            f: ::plum_wasm::__rt::js_sys::Function,
        ) -> ::core::result::Result<u32, ::wasm_bindgen::JsValue> {
            let (core, bridge) = self.__plum();
            #conversions
            #body
        }
    })
}

/// "isEditing" -> "watchIsEditing".
fn watch_name(js: &str) -> String {
    let mut chars = js.chars();
    match chars.next() {
        Some(c) => format!("watch{}{}", c.to_ascii_uppercase(), chars.as_str()),
        None => String::from("watch"),
    }
}

/// The TypeScript for one `#[plum(watch)]` method. Without parameters it is
/// a store; with parameters it is a function from arguments to a store. With
/// a setter it is writable.
fn store_signature(
    func: &ImplItemFn,
    js: &str,
    params: &[Param],
    setter: Option<&str>,
) -> TokenStream {
    let ReturnType::Type(_, returned) = &func.sig.output else {
        return TokenStream::new();
    };
    let value = quote_spanned! {returned.span()=>
        <#returned as ::leptos::reactive::traits::Get>::Value
    };
    let watch = watch_name(js);
    let labels: Vec<String> = params
        .iter()
        .map(|p| snake_to_camel(&p.name.to_string()))
        .collect();
    let types = params.iter().map(|p| ts_name(&p.ts));
    let list = labels.join(", ");
    let (kind, make) = match setter {
        Some(_) => ("WritableAtom", "writable"),
        None => ("ReadableAtom", "readable"),
    };
    let setter = setter.unwrap_or_default();
    quote! {
        {
            let value = {
                visitor.visit::<#value>();
                <#value as ::ts_rs::TS>::name(cfg)
            };
            let params: Vec<String> = vec![#(format!("{}: {}", #labels, #types)),*];
            let params = params.join(", ");
            // `key, ` in front of the callback or the value, or nothing.
            let lead = if #list.is_empty() { String::new() } else { format!("{}, ", #list) };
            let write = if #setter.is_empty() {
                String::new()
            } else {
                format!(", (v) => model.{}({lead}v)", #setter)
            };
            let store = format!(
                "{}<{value}>((cb) => model.{}({lead}cb), unwatch{write})",
                #make, #watch
            );
            if #list.is_empty() {
                store_sigs.push_str(&format!("  {}: {}<{value}>;\n", #js, #kind));
                store_fns.push_str(&format!("      {}: {store},\n", #js));
                wasm_sigs.push_str(&format!("  {}(cb: (v: {value}) => void): number;\n", #watch));
            } else {
                store_sigs.push_str(&format!("  {}({params}): {}<{value}>;\n", #js, #kind));
                store_fns.push_str(&format!("      {}: family(({params}) => {store}),\n", #js));
                wasm_sigs.push_str(&format!(
                    "  {}({params}, cb: (v: {value}) => void): number;\n",
                    #watch
                ));
            }
        }
    }
}

/// `create{Model}(..)` from `pub fn new(..) -> Self` (or `-> Result<Self, E>`).
fn factory(func: &ImplItemFn, model: &Ident, wasm: &Ident, params: &[Param]) -> TokenStream {
    let js = format!("create{model}");
    let rust = format_ident!("create_{}", snake_case(&model.to_string()));
    let names = params.iter().map(|p| &p.name);
    let conversions = conversions(&js, params);
    let args = call_args(params);
    let construct = match returned(&func.sig.output) {
        Returned::Fallible(_) => {
            quote!(#model::new(#(#args),*).map_err(::plum_wasm::__rt::err)?)
        }
        _ => quote!(#model::new(#(#args),*)),
    };
    quote! {
        #[cfg(feature = "plum")]
        #[::wasm_bindgen::prelude::wasm_bindgen(js_name = #js)]
        pub fn #rust(
            #(#names: ::wasm_bindgen::JsValue),*
        ) -> ::core::result::Result<#wasm, ::wasm_bindgen::JsValue> {
            // Signals need an owner and async actions an executor.
            ::plum_wasm::runtime::init();
            #conversions
            ::core::result::Result::Ok(#wasm::new_with(#construct))
        }
    }
}

/// Records one action for the TypeScript binding. Runs inside the export
/// test, where ts-rs can name types. A method that writes a store is only
/// declared on the wasm class.
fn signature(func: &ImplItemFn, js: &str, params: &[Param], is_setter: bool) -> TokenStream {
    let labels: Vec<String> = params
        .iter()
        .map(|p| snake_to_camel(&p.name.to_string()))
        .collect();
    let types = params.iter().map(|p| ts_name(&p.ts));
    let ret = match returned(&func.sig.output) {
        Returned::Nothing | Returned::Fallible(None) => quote!(String::from("void")),
        Returned::Value(ty) | Returned::Fallible(Some(ty)) => ts_name(ty),
    };
    let list = labels.join(", ");
    quote! {
        {
            let params: Vec<String> = vec![#(format!("{}: {}", #labels, #types)),*];
            let sig = format!("  {}({}): {};\n", #js, params.join(", "), #ret);
            if #is_setter {
                wasm_sigs.push_str(&sig);
            } else {
                let implementation = format!("      {0}: ({1}) => model.{0}({1}),\n", #js, #list);
                actions.push((#js, sig, implementation));
            }
        }
    }
}

/// An expression that shows `ty` to the visitor and evaluates to its name.
fn ts_name(ty: &Type) -> TokenStream {
    quote_spanned! {ty.span()=> {
        visitor.visit::<#ty>();
        <#ty as ::ts_rs::TS>::name(cfg)
    }}
}

enum Returned<'a> {
    /// No return type, or `()`.
    Nothing,
    Value(&'a Type),
    /// `Result<T, E>`: `Err` is thrown. `None` when `T` is `()`.
    Fallible(Option<&'a Type>),
}

fn returned(output: &ReturnType) -> Returned<'_> {
    let ReturnType::Type(_, ty) = output else {
        return Returned::Nothing;
    };
    if is_unit(ty) {
        return Returned::Nothing;
    }
    if let Type::Path(tp) = &**ty {
        if let Some(segment) = tp.path.segments.last() {
            if segment.ident == "Result" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(ok)) = args.args.first() {
                        return Returned::Fallible((!is_unit(ok)).then_some(ok));
                    }
                }
            }
        }
    }
    Returned::Value(ty)
}

fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(t) if t.elems.is_empty())
}

fn returns_handle(output: &ReturnType) -> bool {
    let ReturnType::Type(_, ty) = output else {
        return false;
    };
    matches!(&**ty, Type::Path(tp)
        if tp.path.segments.last().is_some_and(|s| HANDLES.contains(&s.ident.to_string().as_str())))
}
