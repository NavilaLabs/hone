//! Tokens of contexts in the OS keyring (concept Section 12, "Contexts").

// One entry per context and server, so that a token stored for one server is never sent to
// another after the context's server changes.
const SERVICE: &str = "hone";

fn entry(context: &str, server: &str) -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, &format!("{context} {server}"))
}

pub(crate) fn store(context: &str, server: &str, token: &str) -> Result<(), String> {
    entry(context, server)
        .and_then(|entry| entry.set_password(token))
        .map_err(|error| format!("the OS keyring cannot store the token: {error}"))
}

pub(crate) fn remove(context: &str, server: &str) -> Result<bool, String> {
    match entry(context, server).and_then(|entry| entry.delete_credential()) {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(format!("the OS keyring cannot remove the token: {error}")),
    }
}

/// The token of the context `context` for `server`: the environment variable `variable` if
/// it is set and not empty, otherwise the token stored in the OS keyring.
pub(crate) fn token(variable: &str, context: &str, server: &str) -> Result<String, String> {
    choose(std::env::var(variable).ok(), variable, context, || {
        match entry(context, server).and_then(|entry| entry.get_password()) {
            Ok(token) => Ok(Some(token)),
            // Without any keyring, for example on a server without a desktop session, there is
            // simply no stored token.
            Err(keyring::Error::NoEntry | keyring::Error::NoDefaultStore) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    })
}

fn choose(
    environment: Option<String>,
    variable: &str,
    context: &str,
    stored: impl FnOnce() -> Result<Option<String>, String>,
) -> Result<String, String> {
    if let Some(token) = environment.filter(|token| !token.is_empty()) {
        return Ok(token);
    }
    let missing = format!(
        "no token for the server: set {variable} or store one with hone context login {context}"
    );
    match stored() {
        Ok(Some(token)) if !token.is_empty() => Ok(token),
        Ok(_) => Err(missing),
        Err(error) => Err(format!(
            "{missing} (the OS keyring cannot be read: {error})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::choose;

    #[test]
    fn the_environment_wins_over_the_keyring() {
        let chosen = choose(Some("from-env".into()), "HONE_TOKEN", "acme", || {
            panic!("the keyring is not asked")
        });
        assert_eq!(chosen.as_deref(), Ok("from-env"));
    }

    #[test]
    fn an_empty_variable_falls_back_to_the_keyring() {
        let chosen = choose(Some(String::new()), "HONE_TOKEN", "acme", || {
            Ok(Some("stored".into()))
        });
        assert_eq!(chosen.as_deref(), Ok("stored"));
    }

    #[test]
    fn without_any_token_the_message_names_both_ways() {
        let missing = choose(None, "HONE_TOKEN_ACME", "acme", || Ok(None));
        assert_eq!(
            missing.as_deref().map_err(String::as_str),
            Err(
                "no token for the server: set HONE_TOKEN_ACME or store one with hone context login acme"
            )
        );
        let unreadable = choose(None, "HONE_TOKEN_ACME", "acme", || Err("locked".into()));
        assert_eq!(
            unreadable.as_deref().map_err(String::as_str),
            Err(
                "no token for the server: set HONE_TOKEN_ACME or store one with hone context login acme (the OS keyring cannot be read: locked)"
            )
        );
        let empty = choose(None, "HONE_TOKEN_ACME", "acme", || Ok(Some(String::new())));
        assert!(empty.is_err(), "an empty stored token is no token");
    }
}
