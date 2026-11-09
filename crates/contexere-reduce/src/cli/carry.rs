//! `contexere carry list`: the MCP servers each installed host has configured, so a
//! new machine knows what to set up (#1092). Names only, never a value: env vars and
//! headers are listed by name, and a URL loses its query and any token-shaped path
//! segment.

use serde::Serialize;
use std::path::Path;

const FLAGS: super::Flags = &[("--json", "Machine-readable JSON output")];

#[derive(Serialize, Debug, PartialEq)]
pub struct ServerRow {
    pub host: String,
    pub server: String,
    /// `stdio`, `http`, `unknown`, or `error` when the file did not parse.
    pub transport: &'static str,
    /// The command, the URL, or for an error row what went wrong.
    pub target: String,
    /// Names of the env vars and headers the server uses, never their values.
    pub env: Vec<String>,
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    super::check_flags("carry", args, FLAGS)?;
    if super::wants_help(args) || args.get(2).map(String::as_str) != Some("list") {
        println!("\ncontexere carry list: the MCP servers your agents use, without secrets\n");
        println!("USAGE:\n  contexere carry list [FLAGS]\n");
        super::print_flags(FLAGS);
        return Ok(());
    }

    let rows: Vec<ServerRow> = crate::agents::all_integrations()
        .iter()
        .filter_map(|host| {
            let (path, key) = host.mcp_config()?;
            Some(rows_for(host.name(), &path, key))
        })
        .flatten()
        .collect();

    if super::has_flag(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        print!("{}", render(&rows));
    }
    Ok(())
}

/// The servers in one host's config file. An absent file, or one without `key`,
/// gives no rows; a file that does not parse gives one error row.
pub fn rows_for(host: &str, path: &Path, key: &str) -> Vec<ServerRow> {
    let error = |what: &str| {
        vec![ServerRow {
            host: host.to_string(),
            server: String::new(),
            transport: "error",
            target: format!("{} {what}", path.display()),
            env: Vec::new(),
        }]
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => return error(&format!("could not be read ({})", e.kind())),
    };
    if text.trim().is_empty() {
        return Vec::new();
    }
    // The parser's own message is left out: a TOML error quotes the offending line,
    // which may be the one holding a key.
    let parsed = if path.extension().is_some_and(|e| e == "toml") {
        toml::from_str::<toml::Value>(&text)
            .ok()
            .and_then(|v| serde_json::to_value(v).ok())
    } else {
        serde_json::from_str(&text)
            .or_else(|_| serde_json::from_str(&crate::agents::plain_json(&text)))
            .ok()
    };
    let Some(config) = parsed else {
        return error("does not parse");
    };
    let Some(servers) = config.get(key).and_then(|s| s.as_object()) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter(|(name, server)| !is_ours(name, server))
        .map(|(name, server)| {
            let (transport, target) = match (url_of(server), command_of(server)) {
                (Some(url), _) => ("http", bare_url(url)),
                (None, Some(cmd)) => ("stdio", cmd.to_string()),
                (None, None) => ("unknown", String::new()),
            };
            ServerRow {
                host: host.to_string(),
                server: name.clone(),
                transport,
                target,
                env: env_names(server),
            }
        })
        .collect()
}

fn is_ours(name: &str, server: &serde_json::Value) -> bool {
    name == crate::agents::SERVER_KEY
        || (name == "omni" && command_of(server).is_some_and(crate::agents::runs_omni_binary))
}

/// `command` as a string, or Zed's `{"path": ...}` form.
fn command_of(server: &serde_json::Value) -> Option<&str> {
    let command = server.get("command")?;
    command.as_str().or_else(|| command.get("path")?.as_str())
}

fn url_of(server: &serde_json::Value) -> Option<&str> {
    ["url", "httpUrl", "serverUrl"]
        .iter()
        .find_map(|k| server.get(*k)?.as_str())
}

/// Keys of the env and header maps, plus the env var names Codex lists directly.
fn env_names(server: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for map in ["env", "headers", "http_headers", "env_http_headers"] {
        if let Some(obj) = server.get(map).and_then(|m| m.as_object()) {
            names.extend(obj.keys().cloned());
        }
    }
    if let Some(list) = server.get("env_vars").and_then(|l| l.as_array()) {
        names.extend(list.iter().filter_map(|n| n.as_str()).map(String::from));
    }
    if let Some(name) = server.get("bearer_token_env_var").and_then(|n| n.as_str()) {
        names.push(name.to_string());
    }
    names.sort();
    names.dedup();
    names
}

/// A URL without its credentials: no userinfo, no query or fragment, and a path
/// segment that looks like a token (a long run mixing letters and digits) hidden.
fn bare_url(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or_default();
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let path: Vec<&str> = path
        .split('/')
        .map(|seg| {
            if looks_like_token(seg) {
                "[REDACTED]"
            } else {
                seg
            }
        })
        .collect();
    let mut out = if scheme.is_empty() {
        host.to_string()
    } else {
        format!("{scheme}://{host}")
    };
    if rest.contains('/') {
        out.push('/');
        out.push_str(&path.join("/"));
    }
    out
}

fn looks_like_token(seg: &str) -> bool {
    seg.len() >= crate::guard::limits::MIN_SECRET_RUN
        && seg.bytes().any(|b| b.is_ascii_digit())
        && seg.bytes().any(|b| b.is_ascii_alphabetic())
}

pub fn render(rows: &[ServerRow]) -> String {
    if rows.is_empty() {
        return "No MCP servers found besides Contexere's own.\n".to_string();
    }
    let host_w = rows.iter().map(|r| r.host.len()).max().unwrap_or(0);
    let name_w = rows.iter().map(|r| r.server.len()).max().unwrap_or(0);
    let mut out = String::new();
    for r in rows {
        let line = format!(
            "{:<host_w$}  {:<name_w$}  {:<7}  {}",
            r.host, r.server, r.transport, r.target
        );
        out.push_str(line.trim_end());
        if !r.env.is_empty() {
            out.push_str(&format!("  env: {}", r.env.join(", ")));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentIntegration;

    fn rows(file: &str, text: &str, key: &str) -> Vec<ServerRow> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(file);
        std::fs::write(&path, text).expect("fixture");
        rows_for("host", &path, key)
    }

    fn summary(rows: &[ServerRow]) -> Vec<String> {
        rows.iter()
            .map(|r| {
                format!(
                    "{} {} {} [{}]",
                    r.server,
                    r.transport,
                    r.target,
                    r.env.join(",")
                )
            })
            .collect()
    }

    fn key_of(host: &dyn AgentIntegration) -> &'static str {
        host.mcp_config().expect("reads MCP config").1
    }

    #[test]
    fn claude_code_lists_its_servers_and_skips_ours() {
        let key = key_of(&crate::agents::ClaudeIntegration);
        let text = r#"{"mcpServers":{
            "contexere":{"command":"/x/contexere","args":["--mcp"]},
            "omni":{"command":"/opt/homebrew/bin/omni"},
            "github":{"command":"npx","args":["-y","@x/github"],"env":{"GITHUB_TOKEN":"v"}},
            "linear":{"type":"http","url":"https://mcp.linear.app/mcp","headers":{"Authorization":"v"}}
        }}"#;
        assert_eq!(
            summary(&rows(".claude.json", text, key)),
            [
                "github stdio npx [GITHUB_TOKEN]",
                "linear http https://mcp.linear.app/mcp [Authorization]"
            ]
        );
    }

    #[test]
    fn codex_lists_its_toml_servers() {
        let key = key_of(&crate::agents::CodexIntegration);
        let text = "[mcp_servers.contexere]\ncommand = \"/x/contexere\"\n\n\
            [mcp_servers.docs]\ncommand = \"docs-mcp\"\nenv_vars = [\"DOCS_KEY\"]\n[mcp_servers.docs.env]\nREGION = \"v\"\n\n\
            [mcp_servers.figma]\nurl = \"https://mcp.figma.com/mcp\"\nbearer_token_env_var = \"FIGMA_TOKEN\"\n";
        assert_eq!(
            summary(&rows("config.toml", text, key)),
            [
                "docs stdio docs-mcp [DOCS_KEY,REGION]",
                "figma http https://mcp.figma.com/mcp [FIGMA_TOKEN]"
            ]
        );
    }

    #[test]
    fn cursor_lists_its_servers() {
        let key = key_of(&crate::agents::CursorIntegration);
        let text = r#"{"mcpServers":{"sentry":{"url":"https://mcp.sentry.dev/sse"}}}"#;
        assert_eq!(
            summary(&rows("mcp.json", text, key)),
            ["sentry http https://mcp.sentry.dev/sse []"]
        );
    }

    #[test]
    fn gemini_lists_http_url_servers() {
        let key = key_of(&crate::agents::GeminiIntegration);
        let text = r#"{"theme":"x","mcpServers":{"search":{"httpUrl":"https://s.example.com/mcp","headers":{"X-Api-Key":"v"}}}}"#;
        assert_eq!(
            summary(&rows("settings.json", text, key)),
            ["search http https://s.example.com/mcp [X-Api-Key]"]
        );
    }

    #[test]
    fn no_secret_value_reaches_either_output() {
        let secret = "sk-ant-api03-9f86d081884c7d659a2feaeb0c55e0f1";
        let text = format!(
            r#"{{"mcpServers":{{
                "a":{{"command":"tool","args":["--key","{secret}"],"env":{{"API_KEY":"{secret}"}}}},
                "b":{{"url":"https://user:{secret}@x.example.com/{secret}/mcp?token={secret}","headers":{{"Authorization":"Bearer {secret}"}}}}
            }}}}"#
        );
        let found = rows("mcp.json", &text, "mcpServers");
        assert_eq!(found.len(), 2);
        let json = serde_json::to_string(&found).expect("json");
        let shown = render(&found);
        for out in [&json, &shown] {
            assert!(!out.contains("9f86d081"), "a secret leaked: {out}");
            assert!(out.contains("API_KEY") && out.contains("Authorization"));
        }
        assert!(
            shown.contains("https://x.example.com/[REDACTED]/mcp"),
            "{shown}"
        );
    }

    #[test]
    fn a_host_without_mcp_config_prints_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(rows_for("host", &dir.path().join("absent.json"), "mcpServers").is_empty());
        assert!(rows("settings.json", r#"{"theme":"dark"}"#, "mcpServers").is_empty());
        assert!(rows("settings.json", "  \n", "mcpServers").is_empty());
    }

    #[test]
    fn a_config_that_does_not_parse_gives_an_error_row() {
        for (file, text) in [
            ("mcp.json", r#"{"mcpServers":{"a":"#),
            (
                "config.toml",
                "[mcp_servers.a]\ncommand = \"sk-ant-secretvalue123456\" oops\n",
            ),
        ] {
            let found = rows(file, text, "mcpServers");
            assert_eq!(found.len(), 1, "{file}");
            assert_eq!(found[0].transport, "error");
            assert!(found[0].target.ends_with("does not parse"));
            assert!(!found[0].target.contains("secretvalue"));
        }
    }

    #[test]
    fn a_config_with_comments_is_still_read() {
        let text = "{\n  // mine\n  \"context_servers\": {\"pg\": {\"command\": {\"path\": \"pg-mcp\"}},},\n}";
        assert_eq!(
            summary(&rows("settings.json", text, "context_servers")),
            ["pg stdio pg-mcp []"]
        );
    }
}
