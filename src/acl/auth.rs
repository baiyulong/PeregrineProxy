use crate::config::UserCredential;

/// Verify username/password against a list of configured credentials
pub fn verify_credentials(username: &str, password: &str, users: &[UserCredential]) -> bool {
    users.iter().any(|u| u.username == username && u.password == password)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UserCredential;

    #[test]
    fn test_verify_credentials_success() {
        let users = vec![
            UserCredential { username: "user1".into(), password: "pass1".into() },
            UserCredential { username: "user2".into(), password: "pass2".into() },
        ];
        
        assert!(verify_credentials("user1", "pass1", &users));
        assert!(verify_credentials("user2", "pass2", &users));
    }
    
    #[test]
    fn test_verify_credentials_failure() {
        let users = vec![
            UserCredential { username: "user1".into(), password: "pass1".into() },
        ];
        
        assert!(!verify_credentials("user1", "wrong", &users));
        assert!(!verify_credentials("wrong", "pass1", &users));
        assert!(!verify_credentials("nonexistent", "anything", &users));
    }
    
    #[test]
    fn test_verify_credentials_empty_list() {
        let users = vec![];
        assert!(!verify_credentials("anyone", "anything", &users));
    }
}