use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::RegexSet;
use std::collections::HashSet;

/// Domain filter that supports both glob patterns and regular expressions
#[derive(Debug, Clone)]
pub struct DomainFilter {
    /// Pre-compiled glob patterns for efficient matching
    glob_set: Option<GlobSet>,
    /// Pre-compiled regex set for efficient matching
    regex_set: Option<RegexSet>,
    /// Store exact domain matches for fastest lookup
    exact_matches: HashSet<String>,
}

impl DomainFilter {
    /// Create a new domain filter from a list of pattern strings
    /// Patterns starting with "~" are treated as regex, others as glob patterns
    pub fn new(patterns: &[String]) -> anyhow::Result<Self> {
        let mut glob_builder = GlobSetBuilder::new();
        let mut regex_patterns = Vec::new();
        let mut exact_matches = HashSet::new();
        let mut has_globs = false;

        for pattern in patterns {
            if let Some(regex_pattern) = pattern.strip_prefix('~') {
                regex_patterns.push(regex_pattern);
            } else if pattern.contains('*') || pattern.contains('?') {
                // Glob pattern
                let glob = Glob::new(pattern)
                    .map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e))?;
                glob_builder.add(glob);
                has_globs = true;
            } else {
                // Exact match - store for O(1) lookup
                exact_matches.insert(pattern.clone());
            }
        }

        let glob_set = if has_globs {
            Some(
                glob_builder
                    .build()
                    .map_err(|e| anyhow::anyhow!("Failed to build glob set: {}", e))?,
            )
        } else {
            None
        };

        let regex_set = if !regex_patterns.is_empty() {
            Some(
                RegexSet::new(&regex_patterns)
                    .map_err(|e| anyhow::anyhow!("Failed to build regex set: {}", e))?,
            )
        } else {
            None
        };

        Ok(DomainFilter {
            glob_set,
            regex_set,
            exact_matches,
        })
    }

    /// Check if a domain matches any of the patterns in this filter
    pub fn matches(&self, domain: &str) -> bool {
        // Check exact matches first (fastest)
        if self.exact_matches.contains(domain) {
            return true;
        }

        // Check glob patterns
        if let Some(ref glob_set) = self.glob_set {
            if glob_set.is_match(domain) {
                return true;
            }
        }

        // Check regex patterns
        if let Some(ref regex_set) = self.regex_set {
            if regex_set.is_match(domain) {
                return true;
            }
        }

        false
    }

    /// Returns true if this filter has no patterns (matches nothing)
    pub fn is_empty(&self) -> bool {
        self.exact_matches.is_empty() && self.glob_set.is_none() && self.regex_set.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let filter = DomainFilter::new(&["example.com".to_string()]).unwrap();
        assert!(filter.matches("example.com"));
        assert!(!filter.matches("test.com"));
        assert!(!filter.matches("sub.example.com"));
    }

    #[test]
    fn test_glob_wildcard() {
        let filter = DomainFilter::new(&["*.example.com".to_string()]).unwrap();
        assert!(filter.matches("sub.example.com"));
        assert!(filter.matches("deep.sub.example.com"));
        assert!(!filter.matches("example.com")); // *.example.com doesn't match exact
        assert!(!filter.matches("notexample.com"));
    }

    #[test]
    fn test_glob_prefix_wildcard() {
        let filter = DomainFilter::new(&["test.*".to_string()]).unwrap();
        assert!(filter.matches("test.com"));
        assert!(filter.matches("test.net"));
        assert!(filter.matches("test.org"));
        assert!(!filter.matches("nottest.com"));
    }

    #[test]
    fn test_regex_pattern() {
        let filter = DomainFilter::new(&["~.*\\.test\\.(com|net)$".to_string()]).unwrap();
        assert!(filter.matches("sub.test.com"));
        assert!(filter.matches("another.test.net"));
        assert!(!filter.matches("test.org"));
        assert!(!filter.matches("nottest.com"));
    }

    #[test]
    fn test_multi_pattern() {
        let patterns = vec![
            "exact.com".to_string(),
            "*.wildcard.com".to_string(),
            "~.*\\.regex\\.net$".to_string(),
        ];
        let filter = DomainFilter::new(&patterns).unwrap();

        assert!(filter.matches("exact.com"));
        assert!(filter.matches("sub.wildcard.com"));
        assert!(filter.matches("sub.regex.net"));
        assert!(!filter.matches("nomatch.com"));
    }

    #[test]
    fn test_empty_filter() {
        let filter = DomainFilter::new(&[]).unwrap();
        assert!(filter.is_empty());
        assert!(!filter.matches("any.domain.com"));
    }

    #[test]
    fn test_invalid_regex() {
        let result = DomainFilter::new(&["~[invalid".to_string()]);
        assert!(result.is_err());
    }
}
