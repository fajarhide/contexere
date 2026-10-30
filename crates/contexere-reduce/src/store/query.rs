use crate::pipeline::SignalTier;
use crate::pipeline::scorer::score_segments;
use crate::store::sqlite::Store;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmniQLQuery {
    ErrorsInLastCommands(usize),
    WarningsFromTool(String),
    ContextForFile(String),
    TimelineToday,
}

/// Slices here are proven to sit on a char boundary; the allow is per function
/// because a file-level one hid a real panic (#619).
#[allow(clippy::string_slice)]
#[allow(clippy::collapsible_if)]
pub fn parse_query(q: &str) -> Option<OmniQLQuery> {
    let q_trimmed = q.trim();
    let q_lower = q_trimmed.to_lowercase();

    if q_lower.starts_with("errors in last ") {
        let parts: Vec<&str> = q_lower.split_whitespace().collect();
        if parts.len() >= 4 {
            if let Ok(n) = parts[3].parse::<usize>() {
                return Some(OmniQLQuery::ErrorsInLastCommands(n));
            }
        }
    } else if q_lower.starts_with("warnings from ") {
        let tool = q_lower.strip_prefix("warnings from ")?.trim().to_string();
        if !tool.is_empty() {
            return Some(OmniQLQuery::WarningsFromTool(tool));
        }
    } else if q_lower.starts_with("context for ") {
        // `get(..12)` on the original, not an offset taken from `to_lowercase()`,
        // which can change the byte length (#621).
        let file = if let Some(prefix) = q_trimmed.get(..12)
            && prefix.eq_ignore_ascii_case("context for ")
        {
            q_trimmed[12..].trim().to_string()
        } else {
            return None;
        };
        if !file.is_empty() {
            return Some(OmniQLQuery::ContextForFile(file));
        }
    } else if q_lower == "timeline today" {
        return Some(OmniQLQuery::TimelineToday);
    }

    None
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OmniQLQueryResult {
    pub query_type: String,
    pub results: Vec<OmniQLRow>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum OmniQLRow {
    #[serde(rename = "error_segment")]
    ErrorSegment {
        command: String,
        timestamp: i64,
        line_range: (usize, usize),
        content: String,
    },
    #[serde(rename = "warning_segment")]
    WarningSegment {
        command: String,
        timestamp: i64,
        line_range: (usize, usize),
        content: String,
    },
    #[serde(rename = "context_match")]
    ContextMatch {
        command: String,
        timestamp: i64,
        matching_lines: Vec<String>,
    },
    #[serde(rename = "timeline_item")]
    TimelineItem {
        timestamp: i64,
        command: String,
        route: String,
        summary: String,
    },
}

impl Store {
    pub fn execute_omni_query(&self, raw_query: &str) -> anyhow::Result<OmniQLQueryResult> {
        let parsed = parse_query(raw_query)
            .ok_or_else(|| anyhow::anyhow!("Invalid OmniQL query grammar. Supported query types:\n  - errors in last N commands\n  - warnings from <tool>\n  - context for <file_path>\n  - timeline today"))?;

        match parsed {
            OmniQLQuery::ErrorsInLastCommands(n) => {
                let mut entries: Vec<(i64, String, String)> = Vec::new();
                {
                    let conn = self.pool.get()?;
                    let mut stmt = conn.prepare(
                        "SELECT ts, command, rewind_hash FROM distillations \
                         WHERE rewind_hash != '' ORDER BY ts DESC LIMIT ?1",
                    )?;
                    let rows = stmt.query_map(
                        params![n as i64],
                        |row| -> Result<(i64, String, String), rusqlite::Error> {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                            ))
                        },
                    )?;
                    for r in rows {
                        entries.push(r?);
                    }
                }

                let mut results = Vec::new();
                for (ts, command, hash) in entries {
                    if let Some(content) = self.retrieve_rewind(&hash) {
                        let segments = score_segments(
                            &content,
                            crate::pipeline::SegmentationMode::Line,
                            None,
                            &command,
                        );
                        for seg in segments {
                            if seg.tier == SignalTier::Critical {
                                results.push(OmniQLRow::ErrorSegment {
                                    command: command.clone(),
                                    timestamp: ts,
                                    line_range: seg.line_range,
                                    content: seg.content,
                                });
                            }
                        }
                    }
                }

                Ok(OmniQLQueryResult {
                    query_type: "errors_in_last_commands".to_string(),
                    results,
                })
            }

            OmniQLQuery::WarningsFromTool(tool) => {
                let mut entries: Vec<(i64, String, String)> = Vec::new();
                {
                    let conn = self.pool.get()?;
                    let mut stmt = conn.prepare(
                        "SELECT ts, command, rewind_hash FROM distillations \
                         WHERE rewind_hash != '' AND (command LIKE ?1 OR filter_name LIKE ?1) \
                         ORDER BY ts DESC LIMIT 50",
                    )?;
                    let rows = stmt.query_map(
                        params![format!("{}%", tool)],
                        |row| -> Result<(i64, String, String), rusqlite::Error> {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                            ))
                        },
                    )?;
                    for r in rows {
                        entries.push(r?);
                    }
                }

                let mut results = Vec::new();
                for (ts, command, hash) in entries {
                    if let Some(content) = self.retrieve_rewind(&hash) {
                        let segments = score_segments(
                            &content,
                            crate::pipeline::SegmentationMode::Line,
                            None,
                            &command,
                        );
                        for seg in segments {
                            if seg.tier == SignalTier::Important {
                                results.push(OmniQLRow::WarningSegment {
                                    command: command.clone(),
                                    timestamp: ts,
                                    line_range: seg.line_range,
                                    content: seg.content,
                                });
                            }
                        }
                    }
                }

                Ok(OmniQLQueryResult {
                    query_type: "warnings_from_tool".to_string(),
                    results,
                })
            }

            OmniQLQuery::ContextForFile(file_path) => {
                let mut matches: Vec<(String, String, i64)> = Vec::new();
                {
                    let conn = self.pool.get()?;
                    let mut stmt = conn.prepare(
                        "SELECT hash, content, ts FROM rewind_store \
                         WHERE content LIKE ?1 ORDER BY ts DESC LIMIT 20",
                    )?;
                    let rows = stmt.query_map(
                        params![format!("%{}%", file_path)],
                        |row| -> Result<(String, String, i64), rusqlite::Error> {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, i64>(2)?,
                            ))
                        },
                    )?;
                    for r in rows {
                        matches.push(r?);
                    }
                }

                let mut results = Vec::new();
                for (hash, content, ts) in matches {
                    let cmd = self
                        .find_command_for_hash(&hash)
                        .unwrap_or_else(|| "unknown".to_string());

                    let mut matching_lines = Vec::new();
                    for line in content.lines() {
                        if line.contains(&file_path) {
                            matching_lines.push(line.to_string());
                        }
                    }

                    if !matching_lines.is_empty() {
                        results.push(OmniQLRow::ContextMatch {
                            command: cmd,
                            timestamp: ts,
                            matching_lines,
                        });
                    }
                }

                Ok(OmniQLQueryResult {
                    query_type: "context_for_file".to_string(),
                    results,
                })
            }

            OmniQLQuery::TimelineToday => {
                let now = chrono::Utc::now().timestamp();
                let midnight = now - (now % 86400);

                let mut entries: Vec<(i64, String, String, i64, i64)> = Vec::new();
                {
                    let conn = self.pool.get()?;
                    let mut stmt = conn.prepare(
                        "SELECT ts, command, route, input_bytes, output_bytes FROM distillations \
                         WHERE ts >= ?1 ORDER BY ts ASC",
                    )?;
                    let rows = stmt.query_map(
                        params![midnight],
                        |row| -> Result<(i64, String, String, i64, i64), rusqlite::Error> {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, i64>(3)?,
                                row.get::<_, i64>(4)?,
                            ))
                        },
                    )?;
                    for r in rows {
                        entries.push(r?);
                    }
                }

                let mut results = Vec::new();
                for (ts, command, route, in_b, out_b) in entries {
                    let summary = if route == "Error" {
                        "Command failed with errors".to_string()
                    } else {
                        let savings = if in_b > 0 {
                            (1.0 - out_b as f64 / in_b as f64) * 100.0
                        } else {
                            0.0
                        };
                        format!("Completed successfully (saved {:.0}% tokens)", savings)
                    };

                    results.push(OmniQLRow::TimelineItem {
                        timestamp: ts,
                        command,
                        route,
                        summary,
                    });
                }

                Ok(OmniQLQueryResult {
                    query_type: "timeline_today".to_string(),
                    results,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_query() {
        assert_eq!(
            parse_query("errors in last 5 commands"),
            Some(OmniQLQuery::ErrorsInLastCommands(5))
        );
        assert_eq!(
            parse_query("warnings from cargo"),
            Some(OmniQLQuery::WarningsFromTool("cargo".to_string()))
        );
        assert_eq!(
            parse_query("context for src/main.rs"),
            Some(OmniQLQuery::ContextForFile("src/main.rs".to_string()))
        );
        assert_eq!(
            parse_query("timeline today"),
            Some(OmniQLQuery::TimelineToday)
        );
        assert_eq!(parse_query("invalid query here"), None);
    }
}
