use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::region::RegionView;

#[derive(Debug, Error)]
pub enum MatcherError {
    #[error("Invalid regex '{pattern}': {source}")]
    InvalidRegex {
        pattern: String,
        #[source]
        source: regex::Error,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatcherNode {
    #[serde(default)]
    pub contains: Vec<String>,
    #[serde(default)]
    pub regex: Vec<String>,
    #[serde(default)]
    pub line_regex: Vec<String>,
    #[serde(default)]
    pub any: Vec<MatcherNode>,
    #[serde(default)]
    pub all: Vec<MatcherNode>,
    #[serde(default)]
    pub not: Vec<MatcherNode>,
}

#[derive(Debug, Clone)]
pub struct CompiledMatcherNode {
    pub contains_lower: Vec<String>,
    pub regex: Vec<Regex>,
    pub line_regex: Vec<Regex>,
    pub any: Vec<CompiledMatcherNode>,
    pub all: Vec<CompiledMatcherNode>,
    pub not: Vec<CompiledMatcherNode>,
}

impl MatcherNode {
    pub fn compile(&self) -> Result<CompiledMatcherNode, MatcherError> {
        let contains_lower = self.contains.iter().map(|s| s.to_lowercase()).collect();
        let mut regex_compiled = Vec::with_capacity(self.regex.len());
        for p in &self.regex {
            let re = Regex::new(p).map_err(|err| MatcherError::InvalidRegex {
                pattern: p.clone(),
                source: err,
            })?;
            regex_compiled.push(re);
        }
        let mut line_regex_compiled = Vec::with_capacity(self.line_regex.len());
        for p in &self.line_regex {
            let re = Regex::new(p).map_err(|err| MatcherError::InvalidRegex {
                pattern: p.clone(),
                source: err,
            })?;
            line_regex_compiled.push(re);
        }
        let any = self
            .any
            .iter()
            .map(|n| n.compile())
            .collect::<Result<Vec<_>, _>>()?;
        let all = self
            .all
            .iter()
            .map(|n| n.compile())
            .collect::<Result<Vec<_>, _>>()?;
        let not = self
            .not
            .iter()
            .map(|n| n.compile())
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CompiledMatcherNode {
            contains_lower,
            regex: regex_compiled,
            line_regex: line_regex_compiled,
            any,
            all,
            not,
        })
    }
}

impl CompiledMatcherNode {
    pub fn matches(&self, view: &RegionView, text_lower: &str) -> bool {
        // 1. contains: ALL substrings must be present (case-insensitive)
        for sub in &self.contains_lower {
            if !text_lower.contains(sub) {
                return false;
            }
        }

        // 2. regex: ALL patterns must match the region text as a whole
        for re in &self.regex {
            if !re.is_match(&view.text) {
                return false;
            }
        }

        // 3. line_regex: ALL listed patterns must each find at least one matching line
        for re in &self.line_regex {
            let found = view.lines.iter().any(|line| re.is_match(line));
            if !found {
                return false;
            }
        }

        // 4. all: every child node matches (AND)
        for child in &self.all {
            if !child.matches(view, text_lower) {
                return false;
            }
        }

        // 5. any: at least one child node matches (OR)
        if !self.any.is_empty() {
            let any_matched = self.any.iter().any(|child| child.matches(view, text_lower));
            if !any_matched {
                return false;
            }
        }

        // 6. not: VETO: if any child matches, this node fails
        for child in &self.not {
            if child.matches(view, text_lower) {
                return false;
            }
        }

        true
    }
}
