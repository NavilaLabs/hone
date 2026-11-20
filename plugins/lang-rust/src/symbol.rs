// Resolving `a::b::c` among the items of a Rust file.

use syn::{Fields, Ident, ImplItem, Item, TraitItem, Type};

/// The line (from 1) of the name of `symbol` in `file`, `None` when the file has no such
/// symbol.
pub(crate) fn line(file: &syn::File, symbol: &str) -> Option<usize> {
    let segments: Vec<&str> = symbol.split("::").collect();
    find(&file.items, &segments).map(|ident| ident.span().start().line)
}

fn find<'a>(items: &'a [Item], segments: &[&str]) -> Option<&'a Ident> {
    let (first, rest) = segments.split_first()?;
    let named = items
        .iter()
        .filter_map(|item| Some((item, ident_of(item)?)))
        .filter(|(_, ident)| *ident == first);
    for (item, ident) in named {
        if rest.is_empty() {
            return Some(ident);
        }
        let found = match item {
            Item::Mod(module) => module
                .content
                .as_ref()
                .and_then(|(_, items)| find(items, rest)),
            Item::Enum(item) if rest.len() == 1 => item
                .variants
                .iter()
                .map(|variant| &variant.ident)
                .find(|ident| *ident == rest[0]),
            Item::Trait(item) if rest.len() == 1 => item
                .items
                .iter()
                .filter_map(|member| match member {
                    TraitItem::Fn(member) => Some(&member.sig.ident),
                    TraitItem::Const(member) => Some(&member.ident),
                    TraitItem::Type(member) => Some(&member.ident),
                    _ => None,
                })
                .find(|ident| *ident == rest[0]),
            Item::Struct(item) if rest.len() == 1 => match &item.fields {
                Fields::Named(fields) => fields
                    .named
                    .iter()
                    .filter_map(|field| field.ident.as_ref())
                    .find(|ident| *ident == rest[0]),
                _ => None,
            },
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    match rest {
        [member] => in_impl(items, first, member),
        _ => None,
    }
}

// An item of an `impl` block of the type `owner`, whether or not the type is defined here.
fn in_impl<'a>(items: &'a [Item], owner: &str, member: &str) -> Option<&'a Ident> {
    items.iter().find_map(|item| {
        let Item::Impl(block) = item else {
            return None;
        };
        let Type::Path(path) = &*block.self_ty else {
            return None;
        };
        if !path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == owner)
        {
            return None;
        }
        block
            .items
            .iter()
            .filter_map(|candidate| match candidate {
                ImplItem::Fn(candidate) => Some(&candidate.sig.ident),
                ImplItem::Const(candidate) => Some(&candidate.ident),
                ImplItem::Type(candidate) => Some(&candidate.ident),
                _ => None,
            })
            .find(|ident| *ident == member)
    })
}

fn ident_of(item: &Item) -> Option<&Ident> {
    let ident = match item {
        Item::Const(item) => &item.ident,
        Item::Enum(item) => &item.ident,
        Item::Fn(item) => &item.sig.ident,
        Item::Macro(item) => item.ident.as_ref()?,
        Item::Mod(item) => &item.ident,
        Item::Static(item) => &item.ident,
        Item::Struct(item) => &item.ident,
        Item::Trait(item) => &item.ident,
        Item::TraitAlias(item) => &item.ident,
        Item::Type(item) => &item.ident,
        Item::Union(item) => &item.ident,
        _ => return None,
    };
    Some(ident)
}
