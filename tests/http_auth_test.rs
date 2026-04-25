use base64::Engine;
use bytes::Bytes;
use http_body_util::Empty;
use hyper::{Method, Request, StatusCode};
use peregrine::config::UserCredential;
use peregrine::protocol::http_handler::{check_proxy_auth, make_407_response};

#[test]
fn test_check_proxy_auth_correct_credentials() {
    let users = vec![
        UserCredential {
            username: "testuser".into(),
            password: "testpass".into(),
        },
        UserCredential {
            username: "admin".into(),
            password: "secret".into(),
        },
    ];

    // Create Basic Auth header: "testuser:testpass" -> base64
    let credentials = "testuser:testpass";
    let base64_creds = base64::engine::general_purpose::STANDARD.encode(credentials);
    let header_value = format!("Basic {}", base64_creds);

    let req = Request::builder()
        .method(Method::GET)
        .uri("http://example.com/")
        .header("proxy-authorization", &header_value)
        .body(Empty::<Bytes>::new())
        .unwrap();

    let result = check_proxy_auth(&req, &users);
    assert!(result, "Should accept valid credentials");
}

#[test]
fn test_check_proxy_auth_wrong_credentials() {
    let users = vec![UserCredential {
        username: "testuser".into(),
        password: "testpass".into(),
    }];

    // Create Basic Auth header with wrong password
    let credentials = "testuser:wrongpass";
    let base64_creds = base64::engine::general_purpose::STANDARD.encode(credentials);
    let header_value = format!("Basic {}", base64_creds);

    let req = Request::builder()
        .method(Method::GET)
        .uri("http://example.com/")
        .header("proxy-authorization", &header_value)
        .body(Empty::<Bytes>::new())
        .unwrap();

    let result = check_proxy_auth(&req, &users);
    assert!(!result, "Should reject invalid credentials");
}

#[test]
fn test_check_proxy_auth_wrong_username() {
    let users = vec![UserCredential {
        username: "testuser".into(),
        password: "testpass".into(),
    }];

    // Create Basic Auth header with wrong username
    let credentials = "wronguser:testpass";
    let base64_creds = base64::engine::general_purpose::STANDARD.encode(credentials);
    let header_value = format!("Basic {}", base64_creds);

    let req = Request::builder()
        .method(Method::GET)
        .uri("http://example.com/")
        .header("proxy-authorization", &header_value)
        .body(Empty::<Bytes>::new())
        .unwrap();

    let result = check_proxy_auth(&req, &users);
    assert!(!result, "Should reject invalid username");
}

#[test]
fn test_check_proxy_auth_missing_header() {
    let users = vec![UserCredential {
        username: "testuser".into(),
        password: "testpass".into(),
    }];

    let req = Request::builder()
        .method(Method::GET)
        .uri("http://example.com/")
        .body(Empty::<Bytes>::new())
        .unwrap();

    let result = check_proxy_auth(&req, &users);
    assert!(!result, "Should reject request without auth header");
}

#[test]
fn test_check_proxy_auth_invalid_header_format() {
    let users = vec![UserCredential {
        username: "testuser".into(),
        password: "testpass".into(),
    }];

    // Invalid header format (not Basic auth)
    let req1 = Request::builder()
        .method(Method::GET)
        .uri("http://example.com/")
        .header("proxy-authorization", "Bearer token123")
        .body(Empty::<Bytes>::new())
        .unwrap();
    let result1 = check_proxy_auth(&req1, &users);
    assert!(!result1, "Should reject non-Basic auth");

    // Invalid base64
    let req2 = Request::builder()
        .method(Method::GET)
        .uri("http://example.com/")
        .header("proxy-authorization", "Basic invalid_base64!")
        .body(Empty::<Bytes>::new())
        .unwrap();
    let result2 = check_proxy_auth(&req2, &users);
    assert!(!result2, "Should reject invalid base64");
}

#[test]
fn test_check_proxy_auth_empty_user_list() {
    let users = vec![];

    let credentials = "anyuser:anypass";
    let base64_creds = base64::engine::general_purpose::STANDARD.encode(credentials);
    let header_value = format!("Basic {}", base64_creds);

    let req = Request::builder()
        .method(Method::GET)
        .uri("http://example.com/")
        .header("proxy-authorization", &header_value)
        .body(Empty::<Bytes>::new())
        .unwrap();

    let result = check_proxy_auth(&req, &users);
    assert!(!result, "Should reject when no users configured");
}

#[tokio::test]
async fn test_make_407_response() {
    let response = make_407_response();

    // Check status code
    assert_eq!(response.status(), StatusCode::PROXY_AUTHENTICATION_REQUIRED);

    // Check Proxy-Authenticate header
    let auth_header = response.headers().get("proxy-authenticate");
    assert!(
        auth_header.is_some(),
        "Should have Proxy-Authenticate header"
    );

    let auth_value = auth_header.unwrap().to_str().unwrap();
    assert_eq!(auth_value, "Basic realm=\"Peregrine Proxy\"");

    // Check body
    let (_parts, body) = response.into_parts();
    let body_bytes = http_body_util::BodyExt::collect(body)
        .await
        .unwrap()
        .to_bytes();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert_eq!(body_str, "Proxy Authentication Required");
}

#[test]
fn test_case_insensitive_header_name() {
    let users = vec![UserCredential {
        username: "testuser".into(),
        password: "testpass".into(),
    }];

    // HTTP header names are case-insensitive
    let credentials = "testuser:testpass";
    let base64_creds = base64::engine::general_purpose::STANDARD.encode(credentials);
    let header_value = format!("Basic {}", base64_creds);

    // Use different case for header name
    let req = Request::builder()
        .method(Method::GET)
        .uri("http://example.com/")
        .header("Proxy-Authorization", &header_value)
        .body(Empty::<Bytes>::new())
        .unwrap();

    let result = check_proxy_auth(&req, &users);
    assert!(result, "Should handle case-insensitive header names");
}
