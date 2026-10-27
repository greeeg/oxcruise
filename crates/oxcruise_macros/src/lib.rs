//! `declare_rule!`: generate the `RuleMeta` impl for a rule struct.
//!
//! ```ignore
//! declare_rule! {
//!     /// ### What it does
//!     /// Reports circular dependencies.
//!     NoCircular, category = correctness, version = "0.1.0", config = NoCircularConfig
//! }
//! ```
//!
//! The rule name is the kebab-case struct name. Doc comments become the rule
//! documentation (website and `--rules` output). `config` names the options
//! struct; without it the rule takes no options.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Expr, ExprLit, Ident, Lit, Meta, Token, Type, parse_macro_input};

struct Declaration {
    docs: Vec<String>,
    name: Ident,
    category: Ident,
    version: String,
    config: Option<Type>,
}

impl Parse for Declaration {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let docs = attrs
            .iter()
            .filter(|a| a.path().is_ident("doc"))
            .filter_map(|a| match &a.meta {
                Meta::NameValue(nv) => match &nv.value {
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) => Some(s.value()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let name: Ident = input.parse()?;

        let (mut category, mut version, mut config) = (None, None, None);
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let key: Ident = input.parse()?;
            match key.to_string().as_str() {
                "category" => {
                    input.parse::<Token![=]>()?;
                    category = Some(input.parse::<Ident>()?);
                }
                "version" => {
                    input.parse::<Token![=]>()?;
                    version = Some(input.parse::<syn::LitStr>()?.value());
                }
                "config" => {
                    input.parse::<Token![=]>()?;
                    config = Some(input.parse::<Type>()?);
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown key `{other}`; expected category, version or config"),
                    ));
                }
            }
        }
        let category =
            category.ok_or_else(|| syn::Error::new(name.span(), "missing `category = ...`"))?;
        let valid = [
            "correctness",
            "suspicious",
            "style",
            "restriction",
            "nursery",
        ];
        if !valid.contains(&category.to_string().as_str()) {
            return Err(syn::Error::new(
                category.span(),
                format!("category must be one of {}", valid.join(", ")),
            ));
        }
        let version =
            version.ok_or_else(|| syn::Error::new(name.span(), "missing `version = \"x.y.z\"`"))?;
        Ok(Declaration {
            docs,
            name,
            category,
            version,
            config,
        })
    }
}

fn kebab(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[proc_macro]
pub fn declare_rule(input: TokenStream) -> TokenStream {
    let Declaration {
        docs,
        name,
        category,
        version,
        config,
    } = parse_macro_input!(input as Declaration);
    let rule_name = kebab(&name.to_string());
    let docs = docs
        .iter()
        .map(|d| d.strip_prefix(' ').unwrap_or(d))
        .collect::<Vec<_>>()
        .join("\n");
    let category_variant = {
        let s = category.to_string();
        let mut c = s.chars();
        let first = c
            .next()
            .expect("non-empty")
            .to_uppercase()
            .collect::<String>();
        Ident::new(&format!("{first}{}", c.as_str()), category.span())
    };
    let config_ty = config.clone().map_or_else(|| quote!(()), |t| quote!(#t));
    let schema = config.map_or_else(
        || quote!(None),
        |t| {
            quote!(Some(
                ::serde_json::to_value(::schemars::schema_for!(#t)).expect("schema serialises")
            ))
        },
    );

    quote! {
        impl crate::meta::RuleMeta for #name {
            const NAME: &'static str = #rule_name;
            const CATEGORY: crate::meta::Category = crate::meta::Category::#category_variant;
            const VERSION: &'static str = #version;
            const DOCS: &'static str = #docs;
            type Config = #config_ty;

            #[cfg(feature = "ruledocs")]
            fn config_schema() -> Option<::serde_json::Value> {
                #schema
            }
        }
    }
    .into()
}
