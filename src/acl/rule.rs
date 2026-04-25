use ipnet::IpNet;
use crate::acl::domain_filter::DomainFilter;
use crate::config::UserCredential;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum AclAction {
    Allow,
    Deny,
    Authenticate,
}

#[derive(Debug, Clone)]
pub struct AclRule {
    pub action: AclAction,
    pub src_ip: Option<Vec<IpNet>>,
    pub dst_domain: Option<DomainFilter>,
    pub http_methods: Option<HashSet<String>>,
    pub auth_users: Option<Vec<UserCredential>>,
}