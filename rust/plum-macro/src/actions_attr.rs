//! `#[plum_actions]`: exports the methods of a model's `impl` block.
//!
//! The block is passed through unchanged. Next to it the macro generates
//!
//! - an action on the wasm class for every `pub fn` that takes `&self`,
//! - a `create{Model}` factory from `pub fn new(..)`, with its parameters,
//! - the TypeScript signatures of those actions, for the generated binding.
//!
//! Every argument arrives as a `JsValue` and is deserialized into the type
//! the method declares, so the macro never has to recognise a type by name.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ImplItem, ImplItemFn, ItemImpl, Pat, ReturnType, Type};

use crate::naming::{snake_case, snake_to_camel, wasm_class_name};

/// Reactive handles. A method that returns one is an accessor for Rust
/// callers, not an action. `#[plum(skip)]` covers anything not listed.
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
    js: Option<String>,
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

    let mut actions = Vec::new();
    let mut signatures = Vec::new();
    let mut factory = TokenStream::new();
    for member in &mut item.items {
        let ImplItem::Fn(func) = member else { continue };
        let options = take_options(func)?;
        if options.skip || !matches!(func.vis, syn::Visibility::Public(_)) {
            continue;
        }
        let name = func.sig.ident.to_string();
        match func.sig.inputs.first() {
            Some(FnArg::Receiver(r)) if r.reference.is_some() && r.mutability.is_none() => {
                if name == "dispose" || returns_handle(&func.sig.output) {
                    continue;
                }
                let js = options.js.unwrap_or_else(|| snake_to_camel(&name));
                let params = params(func)?;
                actions.push(action(func, &js, &params));
                signatures.push(signature(func, &js, &params));
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
            ) -> (String, String) {
                let (mut sigs, mut fns) = (String::new(), String::new());
                #(#signatures)*
                (sigs, fns)
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
        let parsed = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                options.skip = true;
                Ok(())
            } else if meta.path.is_ident("js") {
                options.js = Some(meta.value()?.parse::<syn::LitStr>()?.value());
                Ok(())
            } else {
                Err(meta.error("plum: expected `skip` or `js = \"...\"`"))
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

    let body = if func.sig.asyncness.is_some() {
        // The call returns at once. What the future does shows up in stores.
        let run = match func.sig.output {
            ReturnType::Default => quote!(core.#name(#(#args),*).await;),
            ReturnType::Type(..) => quote!(let _ = core.#name(#(#args),*).await;),
        };
        quote! {
            let core = ::std::rc::Rc::clone(core);
            ::plum_wasm::__rt::any_spawner::Executor::spawn_local(async move { #run });
            #undefined
        }
    } else {
        match returned(&func.sig.output) {
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
        }
    };

    quote! {
        #[::wasm_bindgen::prelude::wasm_bindgen(js_name = #js)]
        pub fn #name(
            &self,
            #(#names: ::wasm_bindgen::JsValue),*
        ) -> ::core::result::Result<::wasm_bindgen::JsValue, ::wasm_bindgen::JsValue> {
            let (core, bridge) = self.__plum();
            let _ = bridge;
            #conversions
            #body
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

/// Appends one action to the TypeScript interface and to the object that
/// implements it. Runs inside the export test, where ts-rs can name types.
fn signature(func: &ImplItemFn, js: &str, params: &[Param]) -> TokenStream {
    let labels: Vec<String> = params
        .iter()
        .map(|p| snake_to_camel(&p.name.to_string()))
        .collect();
    let types = params.iter().map(|p| ts_name(&p.ts));
    let ret = if func.sig.asyncness.is_some() {
        quote!(String::from("void"))
    } else {
        match returned(&func.sig.output) {
            Returned::Nothing | Returned::Fallible(None) => quote!(String::from("void")),
            Returned::Value(ty) | Returned::Fallible(Some(ty)) => ts_name(ty),
        }
    };
    let list = labels.join(", ");
    quote! {
        {
            let params: Vec<String> = vec![#(format!("{}: {}", #labels, #types)),*];
            sigs.push_str(&format!("  {}({}): {};\n", #js, params.join(", "), #ret));
            fns.push_str(&format!("      {0}: ({1}) => model.{0}({1}),\n", #js, #list));
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
