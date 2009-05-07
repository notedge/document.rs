/// Parsed `robots.txt` document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RobotsTxt {
    /// User-agent groups in document order.
    pub groups: Vec<RobotsGroup>,
    /// Top-level `Sitemap` directives.
    pub sitemaps: Vec<String>,
}

/// One `User-agent` block with path rules.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RobotsGroup {
    /// `User-agent` tokens for this block.
    pub agents: Vec<String>,
    /// Allow rules in document order.
    pub allow: Vec<String>,
    /// Disallow rules in document order.
    pub disallow: Vec<String>,
    /// Optional `Crawl-delay` in seconds.
    pub crawl_delay: Option<u32>,
}

impl RobotsTxt {
    /// Parse a `robots.txt` body.
    pub fn parse(body: &str) -> Self {
        let mut document = Self::default();
        let mut current = RobotsGroup::default();
        let mut in_group = false;

        for line in body.lines() {
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = split_directive(line)
            else {
                continue;
            };
            let key = key.to_ascii_lowercase();
            match key.as_str() {
                "user-agent" => {
                    if in_group && !current.agents.is_empty() {
                        document.groups.push(current);
                        current = RobotsGroup::default();
                    }
                    in_group = true;
                    current.agents.push(value.to_string());
                }
                "allow" if in_group => current.allow.push(value.to_string()),
                "disallow" if in_group => current.disallow.push(value.to_string()),
                "crawl-delay" if in_group => {
                    current.crawl_delay = value.parse().ok();
                }
                "sitemap" => document.sitemaps.push(value.to_string()),
                _ => {}
            }
        }

        if in_group && !current.agents.is_empty() {
            document.groups.push(current);
        }
        document
    }

    /// Select the most specific group whose `User-agent` matches the crawler token.
    pub fn group_for_agent(&self, user_agent: &str) -> Option<&RobotsGroup> {
        self.groups
            .iter()
            .filter(|group| group.matches_agent(user_agent))
            .max_by_key(|group| group.agent_specificity(user_agent))
    }

    /// Whether a path is allowed for the given crawler user-agent.
    pub fn is_path_allowed(&self, user_agent: &str, path: &str) -> bool {
        let Some(group) = self.group_for_agent(user_agent)
        else {
            return true;
        };
        group.is_path_allowed(path)
    }

    /// Crawl-delay for the matching group, if present.
    pub fn crawl_delay_for_agent(&self, user_agent: &str) -> Option<u32> {
        self.group_for_agent(user_agent).and_then(|group| group.crawl_delay)
    }
}

impl RobotsGroup {
    fn matches_agent(&self, user_agent: &str) -> bool {
        self.agent_specificity(user_agent) > 0 || self.agents.iter().any(|agent| agent.trim().eq_ignore_ascii_case("*"))
    }

    fn agent_specificity(&self, user_agent: &str) -> usize {
        let ua = user_agent.to_ascii_lowercase();
        self.agents
            .iter()
            .filter_map(|agent| {
                let agent = agent.trim().to_ascii_lowercase();
                if agent == "*" {
                    None
                }
                else if ua.starts_with(&agent) {
                    Some(agent.len())
                }
                else {
                    None
                }
            })
            .max()
            .unwrap_or(0)
    }

    /// Whether `path` is allowed for this robots group.
    pub fn is_path_allowed(&self, path: &str) -> bool {
        let mut best: Option<(bool, usize)> = None;
        for pattern in &self.allow {
            if path_matches_rule(pattern, path) {
                let len = rule_specificity(pattern);
                if best.map(|(_, current)| len > current).unwrap_or(true) {
                    best = Some((true, len));
                }
            }
        }
        for pattern in &self.disallow {
            if path_matches_rule(pattern, path) {
                let len = rule_specificity(pattern);
                if best.map(|(_, current)| len > current).unwrap_or(true) {
                    best = Some((false, len));
                }
            }
        }
        best.map(|(allowed, _)| allowed).unwrap_or(true)
    }
}

fn strip_comment(line: &str) -> &str {
    line.split_once('#').map_or(line, |(head, _)| head)
}

fn split_directive(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim(), value.trim()))
}

fn rule_specificity(pattern: &str) -> usize {
    pattern.trim_end_matches('$').len()
}

/// Match one robots path rule against a URL path.
pub fn path_matches_rule(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    let (pattern, end_anchor) = match pattern.strip_suffix('$') {
        Some(prefix) => (prefix, true),
        None => (pattern, false),
    };
    wildcard_match(pattern, path, end_anchor)
}

fn wildcard_match(pattern: &str, path: &str, end_anchor: bool) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let path: Vec<char> = path.chars().collect();
    let mut pattern_index = 0;
    let mut path_index = 0;
    let mut star_index = None;
    let mut star_match = 0;

    while path_index < path.len() {
        if pattern_index == pattern.len() && !end_anchor {
            return true;
        }
        if pattern_index < pattern.len() && pattern[pattern_index] != '*' && pattern[pattern_index] == path[path_index] {
            pattern_index += 1;
            path_index += 1;
        }
        else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_index = Some(pattern_index);
            star_match = path_index;
            pattern_index += 1;
        }
        else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_match += 1;
            path_index = star_match;
        }
        else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

/// Stable site key for robots caching (`scheme://host:port`).
pub fn site_key_for_url(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or_default();
    let port = url.port_or_known_default().unwrap_or(80);
    format!("{}://{}:{}", url.scheme(), host, port)
}
