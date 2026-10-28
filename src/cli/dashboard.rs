//! A local dashboard over the same `Store` methods `omni stats` reads, so it is
//! never a second source of truth (#451). One thread, one page, std only, no
//! JavaScript, bound to `127.0.0.1` because the data is command output.

use crate::store::sqlite::Store;
use anyhow::Result;
use colored::*;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};

const FLAGS: super::Flags = &[("--port <n>", "Port to bind on 127.0.0.1 (default 7717)")];

const DEFAULT_PORT: u16 = 7717;

fn print_help() {
    println!(
        "\n{} {}: A local dashboard over the numbers omni stats prints",
        "omni".bold().cyan(),
        "dashboard".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  omni {} {}", "dashboard".cyan(), "[FLAGS]".bright_black());
    println!();
    super::print_flags(FLAGS);
    println!("Binds 127.0.0.1 only. Ctrl-C to stop.\n");
}

pub fn run(args: &[String], store: &Store) -> Result<()> {
    if super::has_flag(args, "--help") || super::has_flag(args, "-h") {
        print_help();
        return Ok(());
    }
    super::check_flags("dashboard", args, FLAGS)?;

    // Reads the value half of `--port=8080` (#646).
    let port = super::flag_value(args, "--port")
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    println!(
        "\n  {} http://127.0.0.1:{port}",
        "OMNI dashboard".bold().bright_white()
    );
    println!("  {}\n", "Ctrl-C to stop".bright_black());

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        // One connection at a time: one reader on their own machine.
        if let Err(e) = serve(stream, store) {
            eprintln!("  {} {e}", "request failed:".bright_black());
        }
    }
    Ok(())
}

fn serve(mut stream: TcpStream, store: &Store) -> Result<()> {
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let path = request_path(&line);

    let (status, body) = match path.as_str() {
        "/" => ("200 OK", page(store)),
        // Tells "server up" from "page broken" without rendering.
        "/health" => ("200 OK", "ok".to_string()),
        _ => ("404 Not Found", "not found".to_string()),
    };

    let content_type = if path == "/" {
        "text/html; charset=utf-8"
    } else {
        "text/plain; charset=utf-8"
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()?;
    Ok(())
}

/// The path from a request line, or `/`. Only `GET` is answered.
fn request_path(request_line: &str) -> String {
    let mut parts = request_line.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(path)) => path.split('?').next().unwrap_or("/").to_string(),
        _ => "/nothing".to_string(),
    }
}

/// Minimal escaping for the few values that reach the page from the database.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn page(store: &Store) -> String {
    let (sessions, median, longest, compacted) = store.session_lifetime(0);
    let periods = store.multi_period_stats().unwrap_or_default();
    let reasons = store.passthrough_reasons(0);

    let lifetime = if sessions == 0 {
        "<p class=\"muted\">Not measurable yet: no session has been closed by a host that reports one.</p>".to_string()
    } else {
        format!(
            "<p class=\"big\">{median}</p><p class=\"muted\">commands in the median closed session, {longest} in the longest, across {sessions} sessions. {compacted} ended at a compaction.</p>"
        )
    };

    let mut rows = String::new();
    for (label, calls, _input, _output, raw_tokens, filtered_tokens) in &periods {
        if *calls == 0 && label != "All Time" {
            continue;
        }
        let pct = if *raw_tokens > 0 {
            100.0 * (1.0 - *filtered_tokens as f64 / *raw_tokens as f64)
        } else {
            0.0
        };
        // Same formatter as the CLI (#463).
        rows.push_str(&format!(
            "<tr><td>{}</td><td class=\"n\">{calls}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{pct:.1}%</td></tr>",
            escape(label),
            super::stats::format_compact(*raw_tokens),
            super::stats::format_compact(*filtered_tokens)
        ));
    }

    // Same two calls as the CLI tables.
    let mut commands = String::new();
    if let Ok(raw) = store.filter_breakdown(0) {
        for (name, calls, pct, _) in super::stats::group_and_calculate_stats(raw, 0)
            .iter()
            .filter(|(_, _, pct, _)| *pct > 0.0)
            .take(10)
        {
            commands.push_str(&format!(
                "<tr><td>{}</td><td class=\"n\">{calls}</td><td class=\"n\">{pct:.1}%</td></tr>",
                escape(name)
            ));
        }
    }
    if commands.is_empty() {
        commands.push_str(
            "<tr><td class=\"muted\" colspan=\"3\">no command has saved anything yet</td></tr>",
        );
    }

    let mut agents = String::new();
    if let Ok(rows) = store.get_agent_breakdown(0) {
        let total: u64 = rows.iter().map(|r| r.calls).sum();
        for r in rows.iter().filter(|r| r.calls > 0) {
            let share = if total > 0 {
                100.0 * r.calls as f64 / total as f64
            } else {
                0.0
            };
            let saved = if r.input_bytes > 0 {
                100.0 * (1.0 - r.output_bytes as f64 / r.input_bytes as f64)
            } else {
                0.0
            };
            agents.push_str(&format!(
                "<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{share:.1}%</td><td class=\"n\">{saved:.1}%</td></tr>",
                escape(super::stats::agent_display_name(&r.agent_id)),
                r.calls
            ));
        }
    }
    if agents.is_empty() {
        agents.push_str(
            "<tr><td class=\"muted\" colspan=\"4\">no agent has been recorded yet</td></tr>",
        );
    }

    let mut gates = String::new();
    for (reason, count) in reasons.iter().take(8) {
        gates.push_str(&format!(
            "<tr><td>{}</td><td class=\"n\">{count}</td></tr>",
            escape(reason)
        ));
    }
    if gates.is_empty() {
        gates.push_str("<tr><td class=\"muted\" colspan=\"2\">nothing recorded yet</td></tr>");
    }

    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>OMNI</title>
<style>
:root {{ color-scheme: light dark; }}
body {{ font: 15px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; margin: 0 auto; max-width: 46rem; padding: 2rem 1rem; }}
h1 {{ font-size: 1rem; letter-spacing: .12em; text-transform: uppercase; }}
h2 {{ font-size: .8rem; letter-spacing: .1em; text-transform: uppercase; margin-top: 2.5rem; opacity: .7; }}
.big {{ font-size: 3rem; margin: .2rem 0; }}
.muted {{ opacity: .65; }}
table {{ border-collapse: collapse; width: 100%; }}
td, th {{ padding: .35rem .5rem; text-align: left; border-bottom: 1px solid rgba(128,128,128,.25); }}
.n {{ text-align: right; font-variant-numeric: tabular-nums; }}
footer {{ margin-top: 3rem; font-size: .8rem; opacity: .6; }}
</style></head><body>
<h1>OMNI</h1>
<h2>Session lifetime</h2>
{lifetime}
<h2>Pipeline diagnostic</h2>
<p class="muted">How much one host's tool output shrank. Not a product claim.</p>
<table><tr><th>window</th><th class="n">calls</th><th class="n">tokens in</th><th class="n">tokens out</th><th class="n">saved</th></tr>{rows}</table>
<h2>Top commands</h2>
<table><tr><th>command</th><th class="n">calls</th><th class="n">saved</th></tr>{commands}</table>
<h2>Agent distribution</h2>
<table><tr><th>agent</th><th class="n">calls</th><th class="n">share</th><th class="n">saved</th></tr>{agents}</table>
<h2>Why payloads were passed through</h2>
<table><tr><th>gate</th><th class="n">calls</th></tr>{gates}</table>
<footer>Read-only, from ~/.omni/omni.db. Same figures as <code>omni stats</code>. Reload to refresh.</footer>
</body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_only_get_and_ignores_the_query_string() {
        assert_eq!(request_path("GET / HTTP/1.1"), "/");
        assert_eq!(request_path("GET /health?x=1 HTTP/1.1"), "/health");
        assert_eq!(request_path("POST / HTTP/1.1"), "/nothing");
        assert_eq!(request_path(""), "/nothing");
    }

    #[test]
    fn escapes_what_the_database_hands_it() {
        assert_eq!(escape("<script>&"), "&lt;script&gt;&amp;");
    }

    /// An empty database renders the page, not a zero that reads as a measurement.
    #[test]
    fn says_not_measurable_rather_than_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");

        let html = page(&store);

        assert!(html.contains("Not measurable yet"), "{html}");
        assert!(html.contains("nothing recorded yet"), "{html}");
    }
}
