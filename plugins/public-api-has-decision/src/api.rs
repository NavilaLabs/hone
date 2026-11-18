use std::collections::BTreeSet;

use quote::ToTokens;
use syn::{
    Attribute, Fields, ImplItem, Item, ItemImpl, TraitItem, Visibility, punctuated::Punctuated,
};

// The public items of the Rust file at `path`, each as its normalized tokens with its `pub mod`
// path. A missing file has none.
pub(crate) fn public_items(path: &str) -> Result<BTreeSet<String>, String> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(format!("{path}: {error}")),
    };
    let file = syn::parse_file(&source).map_err(|error| format!("{path}: {error}"))?;
    let mut items = BTreeSet::new();
    collect(&file.items, "", &mut items);
    Ok(items)
}

fn collect(items: &[Item], module: &str, out: &mut BTreeSet<String>) {
    for item in items {
        let mut item = item.clone();
        let public = match &mut item {
            Item::Fn(function) => {
                if is_public(&function.vis) {
                    out.insert(format!("{module}{}", tokens(&function.sig)));
                }
                continue;
            }
            Item::Struct(structure) => {
                if !is_public(&structure.vis) {
                    continue;
                }
                strip(&mut structure.attrs);
                let fields = match &mut structure.fields {
                    Fields::Named(named) => Some(&mut named.named),
                    Fields::Unnamed(unnamed) => Some(&mut unnamed.unnamed),
                    Fields::Unit => None,
                };
                if let Some(fields) = fields {
                    *fields = fields
                        .iter()
                        .filter(|field| is_public(&field.vis))
                        .cloned()
                        .map(|mut field| {
                            strip(&mut field.attrs);
                            field
                        })
                        .collect::<Punctuated<_, _>>();
                }
                true
            }
            Item::Enum(enumeration) => {
                strip(&mut enumeration.attrs);
                for variant in &mut enumeration.variants {
                    strip(&mut variant.attrs);
                }
                is_public(&enumeration.vis)
            }
            Item::Trait(definition) => {
                strip(&mut definition.attrs);
                for member in &mut definition.items {
                    match member {
                        TraitItem::Fn(function) => {
                            strip(&mut function.attrs);
                            function.default = None;
                        }
                        TraitItem::Const(constant) => {
                            strip(&mut constant.attrs);
                            constant.default = None;
                        }
                        TraitItem::Type(alias) => strip(&mut alias.attrs),
                        _ => {}
                    }
                }
                is_public(&definition.vis)
            }
            Item::Type(alias) => {
                strip(&mut alias.attrs);
                is_public(&alias.vis)
            }
            Item::Const(constant) => {
                if is_public(&constant.vis) {
                    let ty = tokens(&constant.ty);
                    out.insert(format!("{module}const {} : {ty}", constant.ident));
                }
                continue;
            }
            Item::Static(value) => {
                if is_public(&value.vis) {
                    let ty = tokens(&value.ty);
                    out.insert(format!("{module}static {} : {ty}", value.ident));
                }
                continue;
            }
            Item::Use(declaration) => {
                strip(&mut declaration.attrs);
                is_public(&declaration.vis)
            }
            Item::Mod(inner) => {
                if is_public(&inner.vis) {
                    out.insert(format!("{module}mod {}", inner.ident));
                    if let Some((_, items)) = &inner.content {
                        collect(items, &format!("{module}{}::", inner.ident), out);
                    }
                }
                continue;
            }
            Item::Impl(block) => {
                collect_impl(block, module, out);
                continue;
            }
            _ => false,
        };
        if public {
            out.insert(format!("{module}{}", tokens(&item)));
        }
    }
}

fn collect_impl(block: &ItemImpl, module: &str, out: &mut BTreeSet<String>) {
    let generics = tokens(&block.generics);
    let target = tokens(&block.self_ty);
    if let Some((negative, path, _)) = &block.trait_ {
        let negative = if negative.is_some() { "!" } else { "" };
        out.insert(format!(
            "{module}impl {generics} {negative}{} for {target}",
            tokens(path)
        ));
        return;
    }
    for member in &block.items {
        if let ImplItem::Fn(function) = member
            && is_public(&function.vis)
        {
            out.insert(format!(
                "{module}impl {generics} {target} {{ {} }}",
                tokens(&function.sig)
            ));
        }
    }
}

fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

// Doc comments are attributes, but they do not change the API.
fn strip(attributes: &mut Vec<Attribute>) {
    attributes.retain(|attribute| !attribute.path().is_ident("doc"));
}

fn tokens(node: &impl ToTokens) -> String {
    node.to_token_stream().to_string()
}
