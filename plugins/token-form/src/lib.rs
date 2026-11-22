//! The token form of a type in a public API entry (`spec/plugins.md`, item 32): the tokens of the
//! source text, with one space between two words and after a comma, no other space, and no comma
//! before a closing bracket.

/// `tokens` in token form, for example `["Map", "<", "K", ",", "V", ",", ">"]` as `Map<K, V>`.
#[must_use]
pub fn join<I>(tokens: I) -> String
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let tokens: Vec<I::Item> = tokens.into_iter().collect();
    let mut out = String::new();
    let mut previous: Option<&str> = None;
    for (index, token) in tokens.iter().enumerate() {
        let token = token.as_ref();
        let closing_next = tokens
            .get(index + 1)
            .is_some_and(|next| matches!(next.as_ref(), ")" | "]" | "}" | ">"));
        if token == "," && closing_next {
            continue;
        }
        if let Some(previous) = previous
            && (previous == "," || (ends_word(previous) && starts_word(token)))
        {
            out.push(' ');
        }
        out.push_str(token);
        previous = Some(token);
    }
    out
}

fn ends_word(token: &str) -> bool {
    token.chars().next_back().is_some_and(is_word)
}

fn starts_word(token: &str) -> bool {
    token.chars().next().is_some_and(is_word)
}

fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use super::join;

    #[test]
    fn spaces_only_between_words_and_after_commas() {
        assert_eq!(join(["Map", "<", "K", ",", "V", ",", ">"]), "Map<K, V>");
        assert_eq!(join(["&", "'", "a", "mut", "T"]), "&'a mut T");
        assert_eq!(join(["A", "|", "B"]), "A|B");
        assert_eq!(join(["$", "x"]), "$x");
        assert_eq!(join(["f", "(", "a", ",", "b", ")"]), "f(a, b)");
        assert_eq!(join(Vec::<&str>::new()), "");
    }
}
