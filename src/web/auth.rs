use axum::extract::{Query, State};
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::web::state::AppState;

#[derive(Deserialize)]
pub struct TokenQuery {
    pub token: Option<String>,
}

pub async fn token_guard(
    State(state): State<AppState>,
    cookies: CookieJar,
    Query(query): Query<TokenQuery>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = state.token.clone() else {
        return Ok(next.run(request).await);
    };
    let presented = query
        .token
        .clone()
        .or_else(|| cookies.get("ins_token").map(|c| c.value().to_string()));
    if let Some(p) = &presented {
        if ct_eq(p.as_bytes(), expected.as_bytes()) {
            let mut resp = next.run(request).await;
            if query.token.is_some() {
                let cookie = format!("ins_token={p}; HttpOnly; Path=/; SameSite=Strict");
                resp.headers_mut()
                    .append(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
            }
            return Ok(resp);
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::ct_eq;

    #[test]
    fn ct_eq_basic() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
    }
}
