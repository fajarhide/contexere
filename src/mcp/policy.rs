//! Which of OMNI's MCP tools a host is told about. An advertised tool is
//! re-read on every request of the session, so each one has to pay for its
//! place: re-measure with `docs/internal/transcript-probes/mcp_surface.py`
//! before adding one. The set follows `Tier`, the same tier `doctor` prints.

use crate::agents::Tier;

/// The tools that pay for their place on a Full-tier host. Measured 2026-08-23
/// over 256 sessions: these two carry 138 of 149 calls in 467 B; the six cut
/// cost 1,631 B for 11 calls (#609). The cut tools are one shell command away
/// here (`omni exec`, `omni remember`, `omni stats --view ...`), or behind
/// `OMNI_MCP_TOOLS=all`.
// `omni_context` became `omni context <file>`: zero calls across 253 sessions.
pub const FULL: &[&str] = &["omni_retrieve", "omni_explain_savings"];
// `omni_context` left before the rule was tightened: it was the only advertised
// tool with zero calls ever across 253 sessions, and the three places OMNI
// recommended it appear zero times in 10,578 traces. The capability moved to
// `omni context <file>` (#609).

/// A Handoff-first host does not rewrite built-in tool output, so MCP is OMNI's
/// only door and an unadvertised tool is unreachable. The widest set, which is
/// also what an unregistered host gets.
pub const HANDOFF: &[&str] = &[
    "omni_retrieve",
    "omni_explain_savings",
    "omni_remember",
    "omni_recall",
    "omni_run",
    "omni_find_noise",
    "omni_context_breakdown",
    "omni_history",
];

/// An MCP-only host has no hook, so the reporting tools would describe nothing.
/// `omni_run` stays: it is the tool that does the distilling (#686). The four
/// reporting tools (803 B) wait until this tier has recorded runs to report on.
pub const MEMORY: &[&str] = &[
    "omni_remember",
    "omni_recall",
    "omni_retrieve",
    "omni_knowledge",
    "omni_run",
];

/// Every tool the binary can serve, so `doctor` can count them without a
/// router. Asserted against the router in `server.rs`.
pub const ALL_TOOL_COUNT: usize = 25;

/// `detect_agent_id` and `AgentIntegration::id` disagree for both Full-tier
/// hosts, hence the alias.
fn integration_id(detected: &str) -> &str {
    match detected {
        "claude_code" => "claude",
        "codex_cli" => "codex",
        other => other,
    }
}

/// The tier the host's integration declares. An unregistered id gets
/// `HandoffFirst`, the widest list: being wrong about a host must cost a flag,
/// not a capability. `McpOnly` as the default once dropped Codex to four tools
/// when `CODEX_SESSION` did not reach the subprocess.
pub fn tier_for(agent_id: &str) -> Tier {
    let want = integration_id(agent_id);
    crate::agents::all_integrations()
        .iter()
        .find(|a| a.id() == want)
        .map_or(Tier::HandoffFirst, |a| a.tier())
}

/// Does `OMNI_MCP_TOOLS` ask for the whole surface? Only `all` does. Split from
/// the environment read so tests never set the variable for real.
pub fn value_means_all(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.eq_ignore_ascii_case("all"))
}

/// The only reader of the variable, so `doctor` and the router agree.
pub fn override_is_on() -> bool {
    value_means_all(std::env::var("OMNI_MCP_TOOLS").ok().as_deref())
}

/// The tools this host is told about.
pub fn active_tools(agent_id: &str) -> &'static [&'static str] {
    match tier_for(agent_id) {
        Tier::Full => FULL,
        Tier::HandoffFirst => HANDOFF,
        Tier::McpOnly => MEMORY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Tier;

    /// Without the alias both Full-tier hosts drop to `McpOnly`.
    #[test]
    fn the_detected_ids_map_to_the_tier_their_integration_declares() {
        assert_eq!(tier_for("claude_code"), Tier::Full);
        assert_eq!(tier_for("codex_cli"), Tier::Full);
        assert_eq!(tier_for("cursor"), Tier::HandoffFirst);
        assert_eq!(tier_for("gemini"), Tier::Full);
    }

    #[test]
    fn an_unregistered_host_keeps_the_widest_set() {
        assert_eq!(tier_for("something-new"), Tier::HandoffFirst);
        assert_eq!(active_tools("something-new"), HANDOFF);
        // `vscode`, `zed` and `antigravity` are left out on purpose: they are
        // registered in `mcp_host::HOSTS` and so are genuinely MCP-only.
        for id in ["windsurf", "vscode_continue", "aider", "terminal"] {
            assert_eq!(active_tools(id), HANDOFF, "{id} lost the widest set");
        }
    }

    /// `FULL` must stay a subset of `HANDOFF`; nothing else keeps them related.
    #[test]
    fn the_priced_set_is_a_subset_of_the_widest_one() {
        for tool in FULL {
            assert!(HANDOFF.contains(tool), "{tool} is in FULL but not HANDOFF");
        }
        assert!(
            FULL.len() < HANDOFF.len(),
            "FULL is meant to be the cut one"
        );
    }

    /// Every tier that cannot rewrite built-in output must offer `omni_run` (#686).
    #[test]
    fn every_hookless_tier_offers_the_tool_that_runs_the_command() {
        for (id, tier) in [
            ("antigravity", Tier::McpOnly),
            ("cursor", Tier::HandoffFirst),
        ] {
            assert_eq!(tier_for(id), tier, "{id} changed tier");
            assert!(
                active_tools(id).contains(&"omni_run"),
                "{id} is {tier:?} and cannot rewrite its host's tool output, so \
                 `omni_run` is the only path by which the model reads less. \
                 Advertised: {:?}",
                active_tools(id)
            );
        }
    }

    /// A tier offering `omni_run` may not be labelled as not distilling.
    #[test]
    fn no_tier_offering_omni_run_claims_it_does_not_distil() {
        for id in ["antigravity", "cursor", "claude_code"] {
            let tier = tier_for(id);
            if active_tools(id).contains(&"omni_run") {
                assert!(
                    !tier.label().contains("no shell distill"),
                    "{id} advertises omni_run and its label denies distilling: {:?}",
                    tier.label()
                );
            }
        }
    }

    /// The reporting tools stay out of `MEMORY` until measured.
    #[test]
    fn the_mcp_only_tier_gains_the_performer_and_not_the_reporters() {
        let tools = active_tools("antigravity");
        for reporter in [
            "omni_explain_savings",
            "omni_find_noise",
            "omni_context_breakdown",
            "omni_history",
        ] {
            assert!(
                !tools.contains(&reporter),
                "{reporter} was added to the MCP-only set without a measurement"
            );
        }
    }

    /// On a Handoff-first host `omni_run` is the only way the model reads less.
    #[test]
    fn a_handoff_first_host_keeps_omni_run() {
        assert!(active_tools("cursor").contains(&"omni_run"));
        // On Full the shell is hooked, so `omni exec` replaces it.
        assert!(!active_tools("claude_code").contains(&"omni_run"));
    }

    /// `cline` takes the trait's default tier, so it is MCP-only.
    #[test]
    fn an_mcp_only_host_gets_the_memory_set_and_nothing_else() {
        let tools = active_tools("cline");
        assert!(tools.contains(&"omni_remember"));
        assert!(tools.contains(&"omni_retrieve"));
        assert!(!tools.contains(&"omni_explain_savings"));
    }

    /// If this list grows, re-run `mcp_surface.py` first.
    #[test]
    fn the_full_set_is_the_ones_that_pay_for_the_prefix() {
        let mut got = active_tools("claude_code").to_vec();
        got.sort_unstable();
        assert_eq!(got, vec!["omni_explain_savings", "omni_retrieve"]);
    }

    #[test]
    fn only_the_documented_spelling_restores_every_tool() {
        assert!(value_means_all(Some("all")));
        assert!(value_means_all(Some("ALL")));
        assert!(!value_means_all(None), "unset must not restore");
        assert!(!value_means_all(Some("")), "empty must not restore");
        assert!(!value_means_all(Some("full")), "undocumented spelling");
    }
}
