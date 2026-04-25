pub mod rule;
pub mod ip_filter;
pub mod auth;

use std::net::IpAddr;
use rule::{AclAction, AclRule};
use crate::config::AccessControlConfig;

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
            
            rules.push(AclRule { action, src_ip });
        }
        
        Ok(Self { default_action, rules })
    }
    
    /// Check if a connection from the given source IP is allowed
    pub fn check_ip(&self, src_ip: &IpAddr) -> AclAction {
        for rule in &self.rules {
            if self.rule_matches_ip(rule, src_ip) {
                return rule.action.clone();
            }
        }
        self.default_action.clone()
    }
    
    fn rule_matches_ip(&self, rule: &AclRule, src_ip: &IpAddr) -> bool {
        // If no src_ip filter, the rule matches all IPs
        match &rule.src_ip {
            Some(networks) => ip_filter::ip_matches(src_ip, networks),
            None => true, // No IP restriction = matches all
        }
    }
}