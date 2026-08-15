const SERVICE: &str = "com.llmwiki.desktop.mineru";
const ACCOUNT: &str = "api-token";

#[cfg(target_os = "macos")]
pub fn store_mineru_token(token: &str) -> Result<(), String> {
    use security_framework::passwords::{delete_generic_password, set_generic_password};

    if token.is_empty() {
        return delete_generic_password(SERVICE, ACCOUNT)
            .or_else(|error| {
                if error.code() == -25300 {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(|error| format!("delete MinerU token from Keychain: {error}"));
    }
    set_generic_password(SERVICE, ACCOUNT, token.as_bytes())
        .map_err(|error| format!("store MinerU token in Keychain: {error}"))
}

#[cfg(target_os = "macos")]
pub fn mineru_token() -> Result<Option<String>, String> {
    use security_framework::passwords::get_generic_password;

    match get_generic_password(SERVICE, ACCOUNT) {
        Ok(value) => String::from_utf8(value)
            .map(Some)
            .map_err(|_| "MinerU token in Keychain is not valid UTF-8".into()),
        Err(error) if error.code() == -25300 => Ok(None),
        Err(error) => Err(format!("read MinerU token from Keychain: {error}")),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn store_mineru_token(_token: &str) -> Result<(), String> {
    Err("system credential storage is not supported on this platform".into())
}

#[cfg(not(target_os = "macos"))]
pub fn mineru_token() -> Result<Option<String>, String> {
    Ok(None)
}

pub fn is_mineru_token_configured() -> bool {
    mineru_token().is_ok_and(|token| token.is_some())
}
