use axum::http::HeaderMap;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::response::Response;

pub const SESSION_COOKIE: &str = "immich_rs_session";
pub const PAIRING_COOKIE: &str = "immich_rs_pairing";

pub fn value(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut found = None;
    for header in headers.get_all(COOKIE) {
        let header_value = header.to_str().ok()?;
        for part in header_value.split(';') {
            let (cookie_name, cookie_value) = part.trim().split_once('=')?;
            if cookie_name == name {
                if found.is_some() || !valid_token(cookie_value) {
                    return None;
                }
                found = Some(cookie_value.to_owned());
            }
        }
    }
    found
}

pub fn append(response: &mut Response, name: &str, value: &str, max_age: u64) -> bool {
    let cookie = format!("{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}");
    let Ok(header) = cookie.parse() else {
        return false;
    };
    response.headers_mut().append(SET_COOKIE, header);
    true
}

pub fn clear(response: &mut Response, name: &str) -> bool {
    let cookie = format!("{name}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    let Ok(header) = cookie.parse() else {
        return false;
    };
    response.headers_mut().append(SET_COOKIE, header);
    true
}

fn valid_token(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
