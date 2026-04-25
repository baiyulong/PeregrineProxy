use ipnet::IpNet;

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
    // domain filter will be added in Task 12
    // http_method filter will be added in Task 21
}