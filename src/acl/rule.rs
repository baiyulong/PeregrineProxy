use ipnet::IpNet;
use crate::acl::domain_filter::DomainFilter;

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
    // http_method filter will be added in Task 21
}