//! What the host says its sessions cost, read from the host's own transcripts (#859).
//!
//! Every other `omni stats` view reports what OMNI did, from OMNI's own tables.
//! This one reads the other side of the boundary: Claude Code writes per-request
//! `usage` and every content block to disk, so the bill and the composition of
//! the context can be stated without taking OMNI's word for anything.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// One tool's share of the tool results the model received.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ToolRow {
    pub name: String,
    pub calls: u64,
    pub result_bytes: u64,
}

/// What one project, model or session spent. Subagent requests are included,
/// because they are billed whether or not they shared the session's context.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Spend {
    pub key: String,
    pub requests: u64,
    pub tokens: u64,
    /// At list price, summed over the requests whose model is in the table.
    pub usd: f64,
    /// Requests on a model the table does not price. Their tokens are counted,
    /// their cost is not, so a row with any of these is a lower bound.
    pub unpriced: u64,
}

/// The host's books for a window. Tokens are billed tokens, bytes are UTF-8 bytes
/// of what entered the context.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Bill {
    pub sessions: u64,
    pub requests: u64,
    pub input_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
    pub tool_result_bytes: u64,
    pub tool_input_bytes: u64,
    pub text_bytes: u64,
    /// Largest first.
    pub tools: Vec<ToolRow>,
    /// Most expensive first. Every request lands in exactly one row of each.
    pub by_model: Vec<Spend>,
    pub by_project: Vec<Spend>,
    pub by_session: Vec<Spend>,
}

impl Bill {
    pub fn tokens(&self) -> u64 {
        self.input_tokens + self.cache_creation_tokens + self.cache_read_tokens + self.output_tokens
    }

    pub fn context_bytes(&self) -> u64 {
        self.tool_result_bytes + self.tool_input_bytes + self.text_bytes
    }

    /// Nothing billed and nothing delivered. A window can hold bytes and no
    /// request, when a request made before it lands its results inside it, so
    /// the request count alone would hide real content. It can also hold only
    /// subagent requests, which reach the spend rows and nothing else.
    pub fn is_empty(&self) -> bool {
        self.requests == 0 && self.context_bytes() == 0 && self.by_model.is_empty()
    }

    /// Every request in the window, subagents included.
    pub fn spend(&self) -> Spend {
        self.by_model.iter().fold(Spend::default(), |mut all, row| {
            all.requests += row.requests;
            all.tokens += row.tokens;
            all.usd += row.usd;
            all.unpriced += row.unpriced;
            all
        })
    }
}

/// The date the prices below were read from Anthropic's published list.
pub const PRICES_AS_OF: &str = "2026-10-06";

/// List price in USD per million tokens. Cache writes are not listed: they are
/// 1.25x input for the 5 minute TTL and 2x for the hour.
struct Price {
    input: f64,
    output: f64,
    cache_read: f64,
}

/// `prompt` is every input token of the request, cached or not, because Haiku 5.5
/// bills the whole request at a higher rate once it passes 100K.
fn price(model: &str, prompt: u64) -> Option<Price> {
    let p = |input, output, cache_read| {
        Some(Price {
            input,
            output,
            cache_read,
        })
    };
    // Sonnet 5 and Haiku 5.5 publish no cache read price of their own; 0.1x
    // input is the general rule the others follow.
    match model {
        "claude-fable-5-1" | "claude-mythos-5-1" => p(10.0, 50.0, 0.25),
        "claude-fable-5" | "claude-mythos-5" => p(10.0, 50.0, 1.0),
        "claude-opus-5-5" => p(4.0, 20.0, 0.20),
        "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6" => {
            p(5.0, 25.0, 0.50)
        }
        "claude-sonnet-5-5" | "claude-sonnet-5" => p(2.0, 10.0, 0.20),
        "claude-sonnet-4-6" => p(3.0, 15.0, 0.30),
        "claude-haiku-5-5" if prompt > 100_000 => p(0.50, 2.50, 0.05),
        "claude-haiku-5-5" => p(0.10, 0.50, 0.01),
        "claude-haiku-4-5" => p(1.0, 5.0, 0.10),
        _ => None,
    }
}

/// The id the price table knows. The host writes `claude-opus-5[1m]` for the
/// long context variant and dated ids like `claude-haiku-4-5-20251001`, and both
/// bill as the bare model.
fn model_id(raw: &str) -> &str {
    let id = raw.split('[').next().unwrap_or(raw);
    match id.rsplit_once('-') {
        Some((head, date)) if date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => id,
    }
}

/// One request's cost at list price, `None` when the model is not in the table.
fn cost(model: &str, usage: &Value) -> Option<f64> {
    let n = |v: &Value| v.as_u64().unwrap_or(0) as f64;
    let input = n(&usage["input_tokens"]);
    let read = n(&usage["cache_read_input_tokens"]);
    let written = n(&usage["cache_creation_input_tokens"]);
    // Without the TTL split every write is priced as 5 minutes, the cheaper one,
    // so the figure errs low rather than high.
    let hour = n(&usage["cache_creation"]["ephemeral_1h_input_tokens"]).min(written);
    let p = price(model, (input + read + written) as u64)?;
    // ponytail: fast mode doubles every rate; a per-class fast price would need
    // a second table, add it if Anthropic ever prices the classes apart.
    let speed = if usage["speed"] == "fast" { 2.0 } else { 1.0 };
    Some(
        speed
            * (input * p.input
                + (written - hour) * p.input * 1.25
                + hour * p.input * 2.0
                + read * p.cache_read
                + n(&usage["output_tokens"]) * p.output)
            / 1e6,
    )
}

fn spend<'a>(rows: &'a mut HashMap<String, Spend>, key: &str) -> &'a mut Spend {
    rows.entry(key.to_string()).or_insert_with(|| Spend {
        key: key.to_string(),
        ..Spend::default()
    })
}

fn ranked(rows: HashMap<String, Spend>) -> Vec<Spend> {
    let mut rows: Vec<Spend> = rows.into_values().collect();
    rows.sort_by(|a, b| {
        b.usd
            .total_cmp(&a.usd)
            .then(b.tokens.cmp(&a.tokens))
            .then(a.key.cmp(&b.key))
    });
    rows
}

/// Where Claude Code keeps its transcripts. `CLAUDE_CONFIG_DIR` is the host's own
/// override, so honouring it reads the same tree the host writes.
pub fn transcripts_root() -> Option<PathBuf> {
    let base = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()?.join(".claude"),
    };
    Some(base.join("projects"))
}

/// Reads every transcript under `root` and keeps what is dated at or after `since`.
///
/// Unreadable files and lines that are not JSON are skipped: a report is not
/// worth failing over one torn line in a file another process is appending to.
pub fn read(root: &Path, since: i64) -> Bill {
    let mut bill = Bill::default();
    let mut sessions: HashSet<String> = HashSet::new();
    let mut tools: HashMap<String, ToolRow> = HashMap::new();
    let mut by_model: HashMap<String, Spend> = HashMap::new();
    let mut by_project: HashMap<String, Spend> = HashMap::new();
    let mut by_session: HashMap<String, Spend> = HashMap::new();

    for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "jsonl") {
            continue;
        }
        // A file last written before the window holds nothing inside it.
        if modified_before(path, since) {
            continue;
        }
        let Ok(file) = File::open(path) else { continue };
        // A request id and a tool_use id are only unique inside one transcript.
        let mut seen_requests: HashSet<String> = HashSet::new();
        let mut tool_of: HashMap<String, String> = HashMap::new();

        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            // A subagent opens its own context, billed apart from the session's.
            // Counting it in the context tables reads as the session resetting
            // its context, so it reaches the spend rows and nothing else.
            let sidechain = record["isSidechain"].as_bool() == Some(true);
            let message = &record["message"];
            if !message.is_object() {
                continue;
            }
            // The host writes one record per content block and repeats the whole
            // `usage` on each, so a request is counted once by id. Only the
            // accounting is gated: the blocks on a repeated record are real and
            // are read below.
            let first_sight = match record["requestId"].as_str() {
                Some(id) => seen_requests.insert(id.to_string()),
                None => true,
            };
            if record["timestamp"]
                .as_str()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .is_some_and(|t| t.timestamp() < since)
            {
                // Outside the window, so nothing here is counted. A call that
                // started before the window can still finish inside it, and its
                // result is matched against the request id above and the tool
                // ids here.
                if sidechain {
                    continue;
                }
                for block in message["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_use"
                        && let (Some(id), Some(name)) =
                            (block["id"].as_str(), block["name"].as_str())
                    {
                        tool_of.insert(id.to_string(), name.to_string());
                    }
                }
                continue;
            }

            let model = message["model"].as_str().unwrap_or("unknown");
            // `<synthetic>` is a message the host wrote itself, never sent to a
            // model, so it has no price and no place in a spend row.
            if first_sight && message["usage"].is_object() && model != "<synthetic>" {
                let usage = &message["usage"];
                let tokens = [
                    "input_tokens",
                    "cache_creation_input_tokens",
                    "cache_read_input_tokens",
                    "output_tokens",
                ]
                .iter()
                .map(|k| usage[*k].as_u64().unwrap_or(0))
                .sum::<u64>();
                let model = model_id(model);
                let usd = cost(model, usage);
                let project = record["cwd"].as_str().unwrap_or("unknown");
                let session = record["sessionId"].as_str().unwrap_or("unknown");
                let session = format!(
                    "{} {}",
                    session.chars().take(8).collect::<String>(),
                    project.rsplit(['/', '\\']).next().unwrap_or(project)
                );
                for row in [
                    spend(&mut by_model, model),
                    spend(&mut by_project, project),
                    spend(&mut by_session, &session),
                ] {
                    row.requests += 1;
                    row.tokens += tokens;
                    match usd {
                        Some(usd) => row.usd += usd,
                        None => row.unpriced += 1,
                    }
                }
            }
            if sidechain {
                continue;
            }
            if let Some(id) = record["sessionId"].as_str() {
                sessions.insert(id.to_string());
            }

            if first_sight && message["usage"].is_object() {
                let usage = &message["usage"];
                bill.requests += 1;
                bill.input_tokens += usage["input_tokens"].as_u64().unwrap_or(0);
                bill.cache_creation_tokens +=
                    usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                bill.cache_read_tokens += usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                bill.output_tokens += usage["output_tokens"].as_u64().unwrap_or(0);
            }

            match &message["content"] {
                Value::String(text) => bill.text_bytes += text.len() as u64,
                Value::Array(blocks) => {
                    for block in blocks {
                        read_block(block, &mut bill, &mut tools, &mut tool_of);
                    }
                }
                _ => {}
            }
        }
    }

    bill.sessions = sessions.len() as u64;
    bill.by_model = ranked(by_model);
    bill.by_project = ranked(by_project);
    bill.by_session = ranked(by_session);
    bill.tools = tools.into_values().collect();
    bill.tools.sort_by(|a, b| {
        b.result_bytes
            .cmp(&a.result_bytes)
            .then(a.name.cmp(&b.name))
    });
    bill
}

fn read_block(
    block: &Value,
    bill: &mut Bill,
    tools: &mut HashMap<String, ToolRow>,
    tool_of: &mut HashMap<String, String>,
) {
    match block["type"].as_str() {
        Some("tool_use") => {
            let name = block["name"].as_str().unwrap_or("unknown").to_string();
            if let Some(id) = block["id"].as_str() {
                tool_of.insert(id.to_string(), name.clone());
            }
            bill.tool_input_bytes += block["input"].to_string().len() as u64;
            row(tools, &name).calls += 1;
        }
        Some("tool_result") => {
            let name = block["tool_use_id"]
                .as_str()
                .and_then(|id| tool_of.get(id))
                .map_or("unknown", String::as_str)
                .to_string();
            let bytes = match &block["content"] {
                Value::String(text) => text.len(),
                Value::Array(parts) => parts
                    .iter()
                    .map(|p| p["text"].as_str().map_or(0, str::len))
                    .sum(),
                Value::Null => 0,
                other => other.to_string().len(),
            } as u64;
            bill.tool_result_bytes += bytes;
            row(tools, &name).result_bytes += bytes;
        }
        Some("text") => bill.text_bytes += block["text"].as_str().map_or(0, str::len) as u64,
        Some("thinking") => {
            bill.text_bytes += block["thinking"].as_str().map_or(0, str::len) as u64;
        }
        _ => {}
    }
}

fn row<'a>(tools: &'a mut HashMap<String, ToolRow>, name: &str) -> &'a mut ToolRow {
    tools.entry(name.to_string()).or_insert_with(|| ToolRow {
        name: name.to_string(),
        ..ToolRow::default()
    })
}

fn modified_before(path: &Path, since: i64) -> bool {
    let Ok(modified) = path.metadata().and_then(|m| m.modified()) else {
        return false;
    };
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .is_ok_and(|d| (d.as_secs() as i64) < since)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const NOW: &str = "2026-10-08T00:00:00.000Z";

    fn transcript(dir: &Path, name: &str, lines: &[String]) {
        let mut file = File::create(dir.join(name)).expect("create the transcript fixture");
        for line in lines {
            writeln!(file, "{line}").expect("write the transcript fixture");
        }
    }

    fn assistant(request: &str, block: &str, extra: &str) -> String {
        format!(
            r#"{{"type":"assistant","sessionId":"s1","timestamp":"{NOW}","requestId":"{request}"{extra},"message":{{"usage":{{"input_tokens":3,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":20}},"content":[{block}]}}}}"#
        )
    }

    fn tool_result(id: &str, content: &str) -> String {
        format!(
            r#"{{"type":"user","sessionId":"s1","timestamp":"{NOW}","message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{content}"}}]}}}}"#
        )
    }

    #[test]
    fn a_request_written_as_two_records_is_billed_once_and_both_blocks_are_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        transcript(
            dir.path(),
            "a.jsonl",
            &[
                assistant("r1", r#"{"type":"text","text":"abcd"}"#, ""),
                assistant(
                    "r1",
                    r#"{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}"#,
                    "",
                ),
                tool_result("t1", "0123456789"),
            ],
        );
        let bill = read(dir.path(), 0);

        assert_eq!(bill.requests, 1, "one request id is one request");
        assert_eq!(bill.cache_read_tokens, 1000);
        assert_eq!(bill.tokens(), 1123);
        // The second record repeats the usage and carries a block of its own.
        assert_eq!(bill.text_bytes, 4);
        assert_eq!(
            bill.tools,
            vec![ToolRow {
                name: "Bash".into(),
                calls: 1,
                result_bytes: 10
            }]
        );
    }

    #[test]
    fn a_subagent_record_is_left_out_of_the_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        transcript(
            dir.path(),
            "a.jsonl",
            &[
                assistant("r1", r#"{"type":"text","text":"main"}"#, ""),
                assistant(
                    "r2",
                    r#"{"type":"text","text":"side"}"#,
                    r#","isSidechain":true"#,
                ),
            ],
        );
        let bill = read(dir.path(), 0);

        assert_eq!(bill.requests, 1);
        assert_eq!(bill.text_bytes, 4);
    }

    #[test]
    fn a_record_dated_before_the_window_is_left_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = assistant("r0", r#"{"type":"text","text":"old"}"#, "")
            .replace(NOW, "2026-01-01T00:00:00.000Z");
        transcript(
            dir.path(),
            "a.jsonl",
            &[old, assistant("r1", r#"{"type":"text","text":"new!"}"#, "")],
        );
        let since = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
            .expect("a literal date")
            .timestamp();
        let bill = read(dir.path(), since);

        assert_eq!(bill.requests, 1);
        assert_eq!(bill.text_bytes, 4);
    }

    #[test]
    fn a_call_that_straddles_the_window_keeps_its_name_and_is_not_billed_twice() {
        let dir = tempfile::tempdir().expect("tempdir");
        let before = "2026-01-01T00:00:00.000Z";
        let call = r#"{"type":"tool_use","id":"t1","name":"Read","input":{}}"#;
        transcript(
            dir.path(),
            "a.jsonl",
            &[
                assistant("r1", call, "").replace(NOW, before),
                // The same request, written again after the boundary.
                assistant("r1", r#"{"type":"text","text":"late"}"#, ""),
                tool_result("t1", "0123456789"),
            ],
        );
        let since = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
            .expect("a literal date")
            .timestamp();
        let bill = read(dir.path(), since);

        assert_eq!(bill.requests, 0, "the request was made before the window");
        assert!(
            !bill.is_empty(),
            "bytes landed in the window, so it is not empty"
        );
        assert!(Bill::default().is_empty());
        assert_eq!(
            bill.tools,
            vec![ToolRow {
                name: "Read".into(),
                calls: 0,
                result_bytes: 10
            }],
            "the result landed in the window, under the tool the transcript names"
        );
    }

    #[test]
    fn a_tool_result_is_counted_in_bytes_under_the_tool_that_produced_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        transcript(
            dir.path(),
            "a.jsonl",
            &[
                assistant(
                    "r1",
                    r#"{"type":"tool_use","id":"t1","name":"Read","input":{}}"#,
                    "",
                ),
                // Three box-drawing characters are nine bytes.
                tool_result("t1", "\u{2500}\u{2500}\u{2500}"),
                tool_result("never-issued", "xy"),
            ],
        );
        let bill = read(dir.path(), 0);

        assert_eq!(bill.tool_result_bytes, 11);
        let read_row = bill
            .tools
            .iter()
            .find(|t| t.name == "Read")
            .expect("a Read row");
        assert_eq!(read_row.result_bytes, 9);
        let orphan = bill
            .tools
            .iter()
            .find(|t| t.name == "unknown")
            .expect("an orphan result is kept, under a name that says so");
        assert_eq!(orphan.result_bytes, 2);
    }

    fn priced(request: &str, model: &str, usage: &str, extra: &str) -> String {
        format!(
            r#"{{"type":"assistant","sessionId":"s1","cwd":"/w/app","timestamp":"{NOW}","requestId":"{request}"{extra},"message":{{"model":"{model}","usage":{usage},"content":[]}}}}"#
        )
    }

    const USAGE: &str = r#"{"input_tokens":1000,"cache_creation_input_tokens":3000,"cache_creation":{"ephemeral_1h_input_tokens":1000},"cache_read_input_tokens":10000,"output_tokens":500}"#;

    #[test]
    fn a_request_is_priced_by_its_model_and_its_cache_writes_by_their_ttl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fast = USAGE.replace(
            r#""output_tokens":500}"#,
            r#""output_tokens":500,"speed":"fast"}"#,
        );
        transcript(
            dir.path(),
            "a.jsonl",
            &[
                priced("r1", "claude-opus-5-5", USAGE, ""),
                priced("r2", "claude-opus-5-5[1m]", &fast, ""),
            ],
        );
        let bill = read(dir.path(), 0);

        // 1000 x $4 + 2000 x $4 x 1.25 + 1000 x $4 x 2 + 10000 x $0.20 + 500 x $20,
        // per million: $0.034, then twice that for the fast request.
        let all = bill.spend();
        assert!((all.usd - 0.102).abs() < 1e-9, "{}", all.usd);
        assert_eq!(all.unpriced, 0);
        assert_eq!(
            bill.by_model.len(),
            1,
            "the [1m] suffix bills as the bare id"
        );
        assert_eq!(bill.by_project[0].key, "/w/app");
        assert_eq!(bill.by_session[0].key, "s1 app");
    }

    #[test]
    fn a_model_the_table_does_not_know_is_unpriced_and_never_free() {
        let dir = tempfile::tempdir().expect("tempdir");
        transcript(
            dir.path(),
            "a.jsonl",
            &[
                priced("r1", "claude-opus-5-5", USAGE, ""),
                priced("r2", "some-new-model", USAGE, ""),
            ],
        );
        let bill = read(dir.path(), 0);

        let unknown = &bill.by_model[1];
        assert_eq!(unknown.key, "some-new-model");
        assert_eq!(
            (unknown.requests, unknown.unpriced, unknown.usd),
            (1, 1, 0.0)
        );
        assert_eq!(unknown.tokens, 14_500, "its tokens are still counted");
        assert_eq!(bill.spend().unpriced, 1);
    }

    #[test]
    fn a_subagent_request_is_spent_but_left_out_of_the_session_tables() {
        let dir = tempfile::tempdir().expect("tempdir");
        transcript(
            dir.path(),
            "a.jsonl",
            &[
                priced("r1", "claude-opus-5-5", USAGE, ""),
                priced("r2", "claude-opus-5-5", USAGE, r#","isSidechain":true"#),
                priced("r3", "<synthetic>", USAGE, ""),
            ],
        );
        let bill = read(dir.path(), 0);

        assert_eq!(
            bill.requests, 2,
            "the synthetic record is the host's, the subagent is not"
        );
        assert_eq!(
            bill.spend().requests,
            2,
            "the subagent is billed, the synthetic is not"
        );
    }

    #[test]
    fn a_window_holding_only_subagent_requests_is_not_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        transcript(
            dir.path(),
            "a.jsonl",
            &[priced(
                "r1",
                "claude-opus-5-5",
                USAGE,
                r#","isSidechain":true"#,
            )],
        );
        let bill = read(dir.path(), 0);

        assert_eq!(bill.requests, 0);
        assert!(
            !bill.is_empty(),
            "its spend would be hidden behind the empty notice"
        );
    }

    #[test]
    fn dated_ids_lose_the_date_and_haiku_bills_long_prompts_higher() {
        assert_eq!(model_id("claude-haiku-4-5-20251001"), "claude-haiku-4-5");
        assert_eq!(model_id("claude-opus-5[1m]"), "claude-opus-5");
        assert_eq!(model_id("claude-opus-5-5"), "claude-opus-5-5");
        let short = price("claude-haiku-5-5", 100_000).expect("priced");
        let long = price("claude-haiku-5-5", 100_001).expect("priced");
        assert_eq!((short.input, long.input), (0.10, 0.50));
    }
}
