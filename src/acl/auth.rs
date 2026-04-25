use crate::config::UserCredential;
use base64::Engine;

/// Verify username/password against a list of configured credentials
pub fn verify_credentials(username: &str, password: &str, users: &[UserCredential]) -> bool {
    users
        .iter()
        .any(|u| u.username == username && u.password == password)
}

/// Parse Basic Auth header value and extract username and password
/// Expected format: "Basic <base64>" where base64 encodes "username:password"
pub fn parse_basic_auth(header_value: &str) -> Option<(String, String)> {
    // Check if header starts with "Basic "
    let header_value = header_value.trim();
    if !header_value.starts_with("Basic ") {
        return None;
    }

    // Extract base64 part
    let base64_part = &header_value[6..]; // Skip "Basic "

    // Decode base64
    let decoded = match base64::engine::general_purpose::STANDARD.decode(base64_part) {
        Ok(decoded) => decoded,
        Err(_) => return None,
    };

    // Convert to string
    let decoded_str = match String::from_utf8(decoded) {
        Ok(s) => s,
        Err(_) => return None,
    };

    // Split on first colon to get username:password
    if let Some(colon_pos) = decoded_str.find(':') {
        let username = decoded_str[..colon_pos].to_string();
        let password = decoded_str[colon_pos + 1..].to_string();
        Some((username, password))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UserCredential;
    use base64::Engine;

    #[test]
    fn test_verify_credentials_success() {
        let users = vec![
            UserCredential {
                username: "user1".into(),
                password: "pass1".into(),
            },
            UserCredential {
                username: "user2".into(),
                password: "pass2".into(),
            },
        ];

        assert!(verify_credentials("user1", "pass1", &users));
        assert!(verify_credentials("user2", "pass2", &users));
    }

    #[test]
    fn test_verify_credentials_failure() {
        let users = vec![UserCredential {
            username: "user1".into(),
            password: "pass1".into(),
        }];

        assert!(!verify_credentials("user1", "wrong", &users));
        assert!(!verify_credentials("wrong", "pass1", &users));
        assert!(!verify_credentials("nonexistent", "anything", &users));
    }

    #[test]
    fn test_verify_credentials_empty_list() {
        let users = vec![];
        assert!(!verify_credentials("anyone", "anything", &users));
    }

    #[test]
    fn test_parse_basic_auth_valid() {
        // "user:pass" -> base64
        let credentials = "user:pass";
        let base64_credentials = base64::engine::general_purpose::STANDARD.encode(credentials);
        let header_value = format!("Basic {}", base64_credentials);

        let result = parse_basic_auth(&header_value);
        assert_eq!(result, Some(("user".to_string(), "pass".to_string())));
    }

    #[test]
    fn test_parse_basic_auth_valid_with_colon_in_password() {
        // "user:pass:word" should split on first colon only
        let credentials = "user:pass:word";
        let base64_credentials = base64::engine::general_purpose::STANDARD.encode(credentials);
        let header_value = format!("Basic {}", base64_credentials);

        let result = parse_basic_auth(&header_value);
        assert_eq!(result, Some(("user".to_string(), "pass:word".to_string())));
    }

    #[test]
    fn test_parse_basic_auth_invalid_prefix() {
        let result = parse_basic_auth("Bearer token123");
        assert_eq!(result, None);

        let result = parse_basic_auth("basic dXNlcjpwYXNz"); // lowercase
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_basic_auth_invalid_base64() {
        let result = parse_basic_auth("Basic invalid_base64!");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_basic_auth_no_colon() {
        // Base64 of "userpassword" (no colon)
        let credentials = "userpassword";
        let base64_credentials = base64::engine::general_purpose::STANDARD.encode(credentials);
        let header_value = format!("Basic {}", base64_credentials);

        let result = parse_basic_auth(&header_value);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_basic_auth_empty_username() {
        // Base64 of ":password"
        let credentials = ":password";
        let base64_credentials = base64::engine::general_purpose::STANDARD.encode(credentials);
        let header_value = format!("Basic {}", base64_credentials);

        let result = parse_basic_auth(&header_value);
        assert_eq!(result, Some(("".to_string(), "password".to_string())));
    }

    #[test]
    fn test_parse_basic_auth_empty_password() {
        // Base64 of "username:"
        let credentials = "username:";
        let base64_credentials = base64::engine::general_purpose::STANDARD.encode(credentials);
        let header_value = format!("Basic {}", base64_credentials);

        let result = parse_basic_auth(&header_value);
        assert_eq!(result, Some(("username".to_string(), "".to_string())));
    }

    #[test]
    fn test_parse_basic_auth_with_whitespace() {
        let credentials = "user:pass";
        let base64_credentials = base64::engine::general_purpose::STANDARD.encode(credentials);
        let header_value = format!("  Basic {}  ", base64_credentials);

        let result = parse_basic_auth(&header_value);
        assert_eq!(result, Some(("user".to_string(), "pass".to_string())));
    }
}
