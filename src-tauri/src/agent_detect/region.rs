use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenInput {
    /// Viewport rows, top to bottom, already reconstructed to plain text.
    pub rows: Vec<String>,
    /// Current OSC 0/1/2 window title.
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Region {
    WholeRecent,
    OscTitle,
    BottomNonEmptyLines(usize),
    TopNonEmptyLines(usize),
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RegionError {
    #[error("Unknown region: '{0}'")]
    UnknownRegion(String),
    #[error("Invalid line count number in region: '{0}'")]
    InvalidNumber(String),
    #[error("Invalid region format: '{0}'")]
    InvalidFormat(String),
}

impl std::str::FromStr for Region {
    type Err = RegionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s == "whole_recent" {
            Ok(Region::WholeRecent)
        } else if s == "osc_title" {
            Ok(Region::OscTitle)
        } else if let Some(rest) = s.strip_prefix("bottom_non_empty_lines(") {
            let num_str = rest
                .strip_suffix(')')
                .ok_or_else(|| RegionError::InvalidFormat(s.to_string()))?;
            let n = num_str
                .trim()
                .parse::<usize>()
                .map_err(|_| RegionError::InvalidNumber(num_str.to_string()))?;
            Ok(Region::BottomNonEmptyLines(n))
        } else if let Some(rest) = s.strip_prefix("top_non_empty_lines(") {
            let num_str = rest
                .strip_suffix(')')
                .ok_or_else(|| RegionError::InvalidFormat(s.to_string()))?;
            let n = num_str
                .trim()
                .parse::<usize>()
                .map_err(|_| RegionError::InvalidNumber(num_str.to_string()))?;
            Ok(Region::TopNonEmptyLines(n))
        } else {
            Err(RegionError::UnknownRegion(s.to_string()))
        }
    }
}

pub struct RegionView<'a> {
    pub lines: Vec<&'a str>,
    pub text: String,
}

impl Region {
    pub fn resolve<'a>(&self, input: &'a ScreenInput) -> RegionView<'a> {
        match self {
            Region::WholeRecent => {
                let lines: Vec<&'a str> = input.rows.iter().map(|s| s.as_str()).collect();
                let text = lines.join("\n");
                RegionView { lines, text }
            }
            Region::OscTitle => RegionView {
                lines: vec![input.title.as_str()],
                text: input.title.clone(),
            },
            Region::BottomNonEmptyLines(n) => {
                let non_empty: Vec<&'a str> = input
                    .rows
                    .iter()
                    .filter(|r| !r.trim().is_empty())
                    .map(|s| s.as_str())
                    .collect();
                let take_count = (*n).min(non_empty.len());
                let lines = if take_count == 0 {
                    Vec::new()
                } else {
                    non_empty[non_empty.len() - take_count..].to_vec()
                };
                let text = lines.join("\n");
                RegionView { lines, text }
            }
            Region::TopNonEmptyLines(n) => {
                let non_empty: Vec<&'a str> = input
                    .rows
                    .iter()
                    .filter(|r| !r.trim().is_empty())
                    .map(|s| s.as_str())
                    .collect();
                let take_count = (*n).min(non_empty.len());
                let lines = if take_count == 0 {
                    Vec::new()
                } else {
                    non_empty[..take_count].to_vec()
                };
                let text = lines.join("\n");
                RegionView { lines, text }
            }
        }
    }
}
