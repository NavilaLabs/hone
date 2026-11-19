// Resolving `a::b::c` among the items of a Rust file.

use syn::{Fields, ImplItem, Item, TraitItem, Type};

pub(crate) fn exists(path: &str, symbol: &str) -> Result<bool, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let file = syn::parse_file(&source).map_err(|error| error.to_string())?;
    let segments: Vec<&str> = symbol.split("::").collect();
    Ok(resolve(&file.items, &segments))
}

fn resolve(items: &[Item], segments: &[&str]) -> bool {
    let Some((first, rest)) = segments.split_first() else {
        return true;
    };
    let named = items
        .iter()
        .filter(|item| name_of(item).as_deref() == Some(*first));
    for item in named {
        if rest.is_empty() {
            return true;
        }
        let found = match item {
            Item::Mod(module) => module
                .content
                .as_ref()
                .is_some_and(|(_, items)| resolve(items, rest)),
            Item::Enum(item) if rest.len() == 1 => {
                item.variants.iter().any(|variant| variant.ident == rest[0])
            }
            Item::Trait(item) if rest.len() == 1 => item.items.iter().any(|member| match member {
                TraitItem::Fn(member) => member.sig.ident == rest[0],
                TraitItem::Const(member) => member.ident == rest[0],
                TraitItem::Type(member) => member.ident == rest[0],
                _ => false,
            }),
            Item::Struct(item) if rest.len() == 1 => match &item.fields {
                Fields::Named(fields) => fields
                    .named
                    .iter()
                    .any(|field| field.ident.as_ref().is_some_and(|ident| ident == rest[0])),
                _ => false,
            },
            _ => false,
        };
        if found {
            return true;
        }
    }
    rest.len() == 1 && in_impl(items, first, rest[0])
}

// An item of an `impl` block of the type `owner`, whether or not the type is defined here.
fn in_impl(items: &[Item], owner: &str, member: &str) -> bool {
    items.iter().any(|item| {
        let Item::Impl(block) = item else {
            return false;
        };
        let Type::Path(path) = &*block.self_ty else {
            return false;
        };
        path.path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == owner)
            && block.items.iter().any(|candidate| match candidate {
                ImplItem::Fn(candidate) => candidate.sig.ident == member,
                ImplItem::Const(candidate) => candidate.ident == member,
                ImplItem::Type(candidate) => candidate.ident == member,
                _ => false,
            })
    })
}

fn name_of(item: &Item) -> Option<String> {
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
    Some(ident.to_string())
}
