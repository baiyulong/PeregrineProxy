use std::net::IpAddr;
use peregrine::acl::AccessController;
use peregrine::acl::rule::AclAction;
use peregrine::config::{AccessControlConfig, AclActionConfig, AclRuleConfig};

fn make_config(default: AclActionConfig, rules: Vec<AclRuleConfig>) -> AccessControlConfig {
    AccessControlConfig {
        default_action: default,
        rules,
    }
}

fn make_rule(action: AclActionConfig, src_ip: Option<Vec<String>>) -> AclRuleConfig {
    AclRuleConfig {
        action,
        src_ip,
        dst_domain: None,
        dst_port: None,
        http_method: None,
        auth: None,
    }
}

#[test]
fn test_default_allow() {
    let config = make_config(AclActionConfig::Allow, vec![]);
    let acl = AccessController::from_config(&config).unwrap();
    let ip: IpAddr = "1.2.3.4".parse().unwrap();
    assert_eq!(acl.check_ip(&ip), AclAction::Allow);
}

#[test]
fn test_default_deny() {
    let config = make_config(AclActionConfig::Deny, vec![]);
    let acl = AccessController::from_config(&config).unwrap();
    let ip: IpAddr = "1.2.3.4".parse().unwrap();
    assert_eq!(acl.check_ip(&ip), AclAction::Deny);
}

#[test]
fn test_allow_specific_cidr() {
    let config = make_config(
        AclActionConfig::Deny,
        vec![make_rule(AclActionConfig::Allow, Some(vec!["192.168.0.0/16".into()]))],
    );
    let acl = AccessController::from_config(&config).unwrap();
    
    assert_eq!(acl.check_ip(&"192.168.1.100".parse().unwrap()), AclAction::Allow);
    assert_eq!(acl.check_ip(&"192.168.255.1".parse().unwrap()), AclAction::Allow);
    assert_eq!(acl.check_ip(&"10.0.0.1".parse().unwrap()), AclAction::Deny); // Falls to default
}

#[test]
fn test_deny_specific_cidr() {
    let config = make_config(
        AclActionConfig::Allow,
        vec![make_rule(AclActionConfig::Deny, Some(vec!["10.0.0.0/8".into()]))],
    );
    let acl = AccessController::from_config(&config).unwrap();
    
    assert_eq!(acl.check_ip(&"10.1.2.3".parse().unwrap()), AclAction::Deny);
    assert_eq!(acl.check_ip(&"192.168.1.1".parse().unwrap()), AclAction::Allow);
}

#[test]
fn test_multiple_cidrs_in_rule() {
    let config = make_config(
        AclActionConfig::Deny,
        vec![make_rule(AclActionConfig::Allow, Some(vec![
            "192.168.0.0/16".into(),
            "10.0.0.0/8".into(),
        ]))],
    );
    let acl = AccessController::from_config(&config).unwrap();
    
    assert_eq!(acl.check_ip(&"192.168.1.1".parse().unwrap()), AclAction::Allow);
    assert_eq!(acl.check_ip(&"10.0.0.1".parse().unwrap()), AclAction::Allow);
    assert_eq!(acl.check_ip(&"172.16.0.1".parse().unwrap()), AclAction::Deny);
}

#[test]
fn test_first_match_wins() {
    let config = make_config(
        AclActionConfig::Deny,
        vec![
            make_rule(AclActionConfig::Deny, Some(vec!["192.168.1.0/24".into()])),
            make_rule(AclActionConfig::Allow, Some(vec!["192.168.0.0/16".into()])),
        ],
    );
    let acl = AccessController::from_config(&config).unwrap();
    
    // 192.168.1.x matches first rule (deny)
    assert_eq!(acl.check_ip(&"192.168.1.100".parse().unwrap()), AclAction::Deny);
    // 192.168.2.x matches second rule (allow)
    assert_eq!(acl.check_ip(&"192.168.2.100".parse().unwrap()), AclAction::Allow);
}

#[test]
fn test_rule_without_ip_matches_all() {
    let config = make_config(
        AclActionConfig::Deny,
        vec![make_rule(AclActionConfig::Allow, None)], // No IP filter = matches all
    );
    let acl = AccessController::from_config(&config).unwrap();
    
    assert_eq!(acl.check_ip(&"1.2.3.4".parse().unwrap()), AclAction::Allow);
    assert_eq!(acl.check_ip(&"10.0.0.1".parse().unwrap()), AclAction::Allow);
}

#[test]
fn test_ipv6_cidr() {
    let config = make_config(
        AclActionConfig::Deny,
        vec![make_rule(AclActionConfig::Allow, Some(vec!["::1/128".into()]))],
    );
    let acl = AccessController::from_config(&config).unwrap();
    
    assert_eq!(acl.check_ip(&"::1".parse().unwrap()), AclAction::Allow);
    assert_eq!(acl.check_ip(&"::2".parse().unwrap()), AclAction::Deny);
}

#[test]
fn test_invalid_cidr_returns_error() {
    let config = make_config(
        AclActionConfig::Allow,
        vec![make_rule(AclActionConfig::Deny, Some(vec!["not-a-cidr".into()]))],
    );
    assert!(AccessController::from_config(&config).is_err());
}