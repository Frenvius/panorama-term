use base64::Engine as _;
use serde_json::Value;

const EMAIL_KEYS: &[&str] = &["email", "email_address", "emailAddress", "mail"];

#[derive(Default)]
struct Found {
    email: Option<String>,
    jwt_email: Option<String>,
    jwt_sub: Option<String>,
}

pub fn jwt_claims(token: &str) -> Option<Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(is_b64url)) {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1].trim_end_matches('='))
        .ok()?;
    serde_json::from_slice::<Value>(&bytes).ok().filter(Value::is_object)
}

fn is_b64url(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'='
}

fn non_empty(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn walk(v: &Value, found: &mut Found) {
    match v {
        Value::Object(map) => {
            for (key, val) in map {
                if found.email.is_none() && EMAIL_KEYS.contains(&key.as_str()) {
                    found.email = non_empty(val).filter(|s| s.contains('@'));
                }
                walk(val, found);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| walk(i, found)),
        Value::String(s) => {
            if let Some(claims) = jwt_claims(s) {
                if found.jwt_email.is_none() {
                    found.jwt_email = claims.get("email").and_then(non_empty);
                }
                if found.jwt_sub.is_none() {
                    found.jwt_sub = claims.get("sub").and_then(non_empty);
                }
            }
        }
        _ => {}
    }
}

pub fn email_of(v: &Value) -> Option<String> {
    let mut found = Found::default();
    walk(v, &mut found);
    found.email.or(found.jwt_email).map(|e| e.to_lowercase())
}

pub fn identity_of(v: &Value) -> Option<String> {
    let mut found = Found::default();
    walk(v, &mut found);
    found
        .email
        .or(found.jwt_email)
        .map(|e| e.to_lowercase())
        .or(found.jwt_sub)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_jwt(payload: &Value) -> String {
        let enc = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!("{}.{}.{}", enc.encode(b"{\"alg\":\"none\"}"), enc.encode(payload.to_string()), enc.encode(b"sig"))
    }

    #[test]
    fn identity_from_nested_email() {
        let v = json!({"a": {"b": [{"emailAddress": "Foo@Example.com"}]}});
        assert_eq!(identity_of(&v).as_deref(), Some("foo@example.com"));
    }

    #[test]
    fn identity_from_jwt_string() {
        let v = json!({"token": {"id_token": make_jwt(&json!({"email":"a@b.co","sub":"123"}))}});
        assert_eq!(identity_of(&v).as_deref(), Some("a@b.co"));
        assert_eq!(email_of(&v).as_deref(), Some("a@b.co"));
    }

    #[test]
    fn identity_falls_back_to_sub() {
        let v = json!({"id_token": make_jwt(&json!({"sub":"user-9"}))});
        assert_eq!(identity_of(&v).as_deref(), Some("user-9"));
        assert_eq!(email_of(&v), None);
    }

    #[test]
    fn identity_none_without_signals() {
        assert_eq!(identity_of(&json!({"access_token": "abc", "x": 1})), None);
    }
}
