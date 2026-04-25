pub mod rule;
pub mod ip_filter;
pub mod auth;
pub mod domain_filter;

use std::net::IpAddr;
use rule::{AclAction, AclRule};
use domain_filter::DomainFilter;
use crate::config::{AccessControlConfig, UserCredential};

pub struct AccessController {
    default_action: AclAction,
    rules: Vec<AclRule>,
}

impl AccessController {
    /// Build from config
    pub fn from_config(config: &AccessControlConfig) -> anyhow::Result<Self> {
        let default_action = match config.default_action {
            crate::config::AclActionConfig::Allow => AclAction::Allow,
            crate::config::AclActionConfig::Deny => AclAction::Deny,
            crate::config::AclActionConfig::Authenticate => AclAction::Authenticate,
        };
        
        let mut rules = Vec::new();
        for rule_cfg in &config.rules {
            let action = match rule_cfg.action {
                crate::config::AclActionConfig::Allow => AclAction::Allow,
                crate::config::AclActionConfig::Deny => AclAction::Deny,
                crate::config::AclActionConfig::Authenticate => AclAction::Authenticate,
            };
            
            let src_ip = if let Some(ref ips) = rule_cfg.src_ip {
                let mut nets = Vec::new();
                for ip_str in ips {
                    let net: ipnet::IpNet = ip_str.parse()
                        .map_err(|e| anyhow::anyhow!("Invalid CIDR '{}': {}", ip_str, e))?;
                    nets.push(net);
                }
                Some(nets)
            } else {
                None
            };

            let dst_domain = if let Some(ref domains) = rule_cfg.dst_domain {
                Some(DomainFilter::new(domains)?)
            } else {
                None
            };

            let auth_users = if let Some(ref auth) = rule_cfg.auth {
                if auth.auth_type == "basic" {
                    auth.users.clone()
                } else {
                    None
                }
            } else {
                None
            };
            
            rules.push(AclRule { action, src_ip, dst_domain, auth_users });
        }
        
        Ok(Self { default_action, rules })
    }
    
    /// Check access with both IP and domain filtering
    /// This is the main method that evaluates all ACL rules
    pub fn check_access(&self, src_ip: &IpAddr, dst_domain: Option<&str>) -> AclAction {
        for rule in &self.rules {
            if self.rule_matches(rule, src_ip, dst_domain) {
                return rule.action.clone();
            }
        }
        self.default_action.clone()
    }
    
    /// Check if a connection from the given source IP is allowed (backward compatibility)
    pub fn check_ip(&self, src_ip: &IpAddr) -> AclAction {
        self.check_access(src_ip, None)
    }
    
    /// Get auth users for the first matching rule that returns Authenticate action
    /// Returns None if no matching rule has Authenticate action or auth users configured
    pub fn get_auth_users_for_rule(&self, src_ip: &IpAddr, dst_domain: Option<&str>) -> Option<&[UserCredential]> {
        for rule in &self.rules {
            if self.rule_matches(rule, src_ip, dst_domain) {
                if rule.action == AclAction::Authenticate {
                    return rule.auth_users.as_ref().map(|users| users.as_slice());
                }
                // If rule matches but action is not Authenticate, no auth needed
                return None;
            }
        }
        
        // Check default action
        if self.default_action == AclAction::Authenticate {
            // For default Authenticate action, we don't have users configured
            // This would be a configuration error, but return empty slice
            Some(&[])
        } else {
            None
        }
    }

    /// Check if a rule matches the given IP and domain
    fn rule_matches(&self, rule: &AclRule, src_ip: &IpAddr, dst_domain: Option<&str>) -> bool {
        // All conditions in a rule must match for the rule to apply (AND logic)
        
        // Check IP filter
        if !self.rule_matches_ip(rule, src_ip) {
            return false;
        }
        
        // Check domain filter
        if !self.rule_matches_domain(rule, dst_domain) {
            return false;
        }
        
        true
    }
    
    fn rule_matches_ip(&self, rule: &AclRule, src_ip: &IpAddr) -> bool {
        // If no src_ip filter, the rule matches all IPs
        match &rule.src_ip {
            Some(networks) => ip_filter::ip_matches(src_ip, networks),
            None => true, // No IP restriction = matches all
        }
    }
    
    fn rule_matches_domain(&self, rule: &AclRule, dst_domain: Option<&str>) -> bool {
        match (&rule.dst_domain, dst_domain) {
            (Some(domain_filter), Some(domain)) => {
                // Rule has domain filter and we have a domain to check
                domain_filter.matches(domain)
            }
            (Some(_), None) => {
                // Rule has domain filter but no domain provided = no match
                false
            }
            (None, _) => {
                // Rule has no domain filter = matches all domains (or no domain)
                true
            }
        }
    }
}