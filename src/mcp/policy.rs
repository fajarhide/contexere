//! Which of VENG's MCP tools a host is told about.
//!
//! An advertised tool sits at position 0 of every request, so it is re-read once
//! per request for the whole session, while a distilled tool result is re-read a
//! median of 64 times and then stops. That is why this list is priced rather than
//! curated: measured 2026-08-15, the 25 tools VENG used to advertise cost 4,940
//! bytes for the 16 nobody ever called, about twice what the distillers earned in
//! the median session that pushed more than 50 KB through the hook.
//!
//! `FULL` carries the current measurement. It is re-run with
//! `docs/internal/transcript-probes/mcp_surface.py`, and a tool added without
//! re-running it has no price behind it.
//!
//! The set is chosen by `Tier`, which already exists and is what `doctor`
//! prints, so this module adds no second opinion about what a host can do.

use crate::agents::Tier;

/// The tools that pay for their place in the prefix of a Full-tier host.
///
/// The old rule was "called at least once in the corpus", and one call is not a
/// price. Re-measured 2026-08-23 over 256 transcripts with
/// `transcript-probes/mcp_surface.py`, the calls are not spread across the set:
///
/// ```text
/// veng_retrieve          207 B   109 calls   24 sessions
/// veng_explain_savings   260 B    29 calls   27 sessions
/// veng_run               417 B     3 calls    2 sessions
/// veng_find_noise        253 B     2 calls    1 session
/// veng_recall            236 B     2 calls    2 sessions
/// veng_context_breakdown 157 B     2 calls    2 sessions
/// veng_remember          332 B     1 call     1 session
/// veng_history           236 B     1 call     1 session
/// ```
///
/// The two kept here carry 138 of the 149 calls in 467 B. The six below cost
/// 1,631 B, 77.7% of the surface, for 11 calls in the life of the corpus, and
/// 214 of the 256 sessions called nothing at all. Every request of every session
/// pays for that, and only 7 sessions ever called one of the six.
///
/// What a cut costs is a door, and only on this tier. `router_from` removes an
/// unadvertised route rather than hiding it, so the tools are gone here until
/// `VENG_MCP_TOOLS=all`; what stands in for them is the shell this tier already
/// hooks. `veng exec` runs what `veng_run` ran, `veng remember` writes what
/// `veng_remember` wrote, `veng stats --view context` prints what
/// `veng_context_breakdown` printed, and `veng stats --view detail` carries
/// `veng_history`'s rows with repeats folded into one line and a count rather
/// than listed per call. `veng_recall` and `veng_find_noise` have no CLI
/// equivalent and sit behind the override alone; they are 4 calls in the whole
/// corpus, and `find_noise` writes TOML filters for a layer #449 retired (#609).
pub const FULL: &[&str] = &["veng_retrieve", "veng_explain_savings"];
// `veng_context` left before the rule was tightened: it was the only advertised
// tool with zero calls ever across 253 sessions, and the three places VENG
// recommended it appear zero times in 10,578 traces. The capability moved to
// `veng context <file>` (#609).

/// A Handoff-first host does not rewrite built-in tool output, so MCP is the
/// only door VENG has there and `FULL`'s pricing does not transfer: an
/// unadvertised tool is not one shell command away, it is unreachable. This is
/// the widest advertised set, and an unregistered host gets it for that reason.
pub const HANDOFF: &[&str] = &[
    "veng_retrieve",
    "veng_explain_savings",
    "veng_remember",
    "veng_recall",
    "veng_run",
    "veng_find_noise",
    "veng_context_breakdown",
    "veng_history",
];

/// An MCP-only host gives VENG no hook, so the tools that *report* on shell
/// distillation would describe something that never happens. `veng_run` is not one
/// of them: it is the tool that performs it, and withholding it was circular. The
/// tier had no distillation partly because the one tool that could produce any was
/// held back on the grounds that the tier had no distillation (#686).
///
/// `HANDOFF` above already makes the argument, and every word of it applies here:
/// MCP is the only door VENG has, `FULL`'s pricing does not transfer because an
/// unadvertised tool is not one shell command away, it is unreachable. The
/// difference between the two tiers is that a Handoff-first host has an installed
/// rule nudging the agent toward `veng_run`. That is a nudge, not a capability.
///
/// Only `veng_run`, not the rest of `HANDOFF`. It costs 417 B in the prefix of
/// every request, and the four reporting tools are 803 B more that this file's own
/// rule says nobody may add without measuring first. They become arguable once
/// there are recorded runs on this tier to report on, which today there are none of.
pub const MEMORY: &[&str] = &[
    "veng_remember",
    "veng_recall",
    "veng_retrieve",
    "veng_knowledge",
    "veng_run",
];

/// Every tool the binary can serve. Kept here so `doctor` does not have to
/// build a router to count them, and asserted against the router in
/// `server.rs` so the two cannot drift.
pub const ALL_TOOL_COUNT: usize = 25;

/// `detect_agent_id` and `AgentIntegration::id` disagree for exactly the two
/// Full-tier hosts, which is the whole reason this function is not a one-liner.
fn integration_id(detected: &str) -> &str {
    match detected {
        "claude_code" => "claude",
        "codex_cli" => "codex",
        other => other,
    }
}

/// The tier the host's own integration declares. `Tier` stays the single source
/// of truth; this only translates the name.
///
/// An id with no registered integration gets the tier carrying the widest list,
/// because being wrong about a host has to cost a flag rather than a capability.
/// `detect_agent_id` can return `windsurf`, `vscode_continue`, `aider` and
/// `terminal`, none of which is registered anywhere; the `McpOnly` default
/// dropped Codex to four tools whenever `CODEX_SESSION` failed to reach the
/// subprocess, since its generated config carries no `VENG_AGENT_ID`. A host that
/// wants the memory set declares it by registering an integration.
///
/// That default was `Full` until `FULL` was priced down to two tools (#609).
/// The guarantee did not change, the list holding it did: `HandoffFirst` is now
/// the one that assumes nothing about whether the host's shell is hooked.
pub fn tier_for(agent_id: &str) -> Tier {
    let want = integration_id(agent_id);
    crate::agents::all_integrations()
        .iter()
        .find(|a| a.id() == want)
        .map_or(Tier::HandoffFirst, |a| a.tier())
}

/// Does `VENG_MCP_TOOLS` ask for the whole surface?
///
/// Split from the environment read so a test can decide this without mutating
/// process environment. Cargo runs tests in parallel, this crate already has
/// tests that set variables, and that combination is how a green local suite
/// went red on CI here. `all` is the only documented spelling; anything else
/// gets the cut set.
pub fn value_means_all(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.eq_ignore_ascii_case("all"))
}

/// The override as the running process sees it. The only place that reads the
/// variable, so `doctor` and the served router cannot disagree about whether the
/// cut is in force.
pub fn override_is_on() -> bool {
    value_means_all(std::env::var("VENG_MCP_TOOLS").ok().as_deref())
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

    /// The trap this module exists for. `detect_agent_id` returns `claude_code`
    /// and `codex_cli`; the integrations that carry the tier are registered as
    /// `claude` and `codex`. A lookup without the alias silently drops both
    /// Full-tier hosts to `McpOnly`, which is the one outcome that would cut
    /// tools the main host actually calls.
    #[test]
    fn the_detected_ids_map_to_the_tier_their_integration_declares() {
        assert_eq!(tier_for("claude_code"), Tier::Full);
        assert_eq!(tier_for("codex_cli"), Tier::Full);
        assert_eq!(tier_for("cursor"), Tier::HandoffFirst);
        assert_eq!(tier_for("gemini"), Tier::Full);
    }

    /// Being wrong about a host must cost a flag, not a capability. Since #609
    /// that means the *widest* list rather than `Full`'s: `FULL` is priced for a
    /// host whose shell VENG hooks, and an unregistered id is exactly the case
    /// where nobody knows whether it has one.
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

    /// `FULL` is a cut of `HANDOFF`, not a different list. Pricing one tier down
    /// must never introduce a tool the tier that depends on MCP does not get,
    /// and the two lists are now written out separately, so nothing but this
    /// keeps them related.
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

    /// #686. The tier with no hook is the one tier where `veng_run` is the only
    /// way VENG can shorten anything, and it was the one tier that did not
    /// advertise it. The reason recorded in the source was circular: no
    /// distillation exists here, so the tool that would create it is withheld.
    ///
    /// Asserted as a property rather than as a list, because a list assertion is
    /// the same hand-maintenance one layer down. Every tier that cannot rewrite
    /// built-in tool output has to offer the tool that runs the command itself.
    #[test]
    fn every_hookless_tier_offers_the_tool_that_runs_the_command() {
        for (id, tier) in [
            ("antigravity", Tier::McpOnly),
            ("cursor", Tier::HandoffFirst),
        ] {
            assert_eq!(tier_for(id), tier, "{id} changed tier");
            assert!(
                active_tools(id).contains(&"veng_run"),
                "{id} is {tier:?} and cannot rewrite its host's tool output, so \
                 `veng_run` is the only path by which the model reads less. \
                 Advertised: {:?}",
                active_tools(id)
            );
        }
    }

    /// #686 made the old label false. `Tier::label()` said "no shell distill" for
    /// a tier that now advertises the tool that distils, and #684 had just put
    /// that sentence in front of the user at install time. Two changes landing in
    /// the same release contradicted each other.
    ///
    /// So the label is checked against the tool set rather than being read once
    /// and trusted: any tier offering `veng_run` may not claim it does not distil.
    #[test]
    fn no_tier_offering_veng_run_claims_it_does_not_distil() {
        for id in ["antigravity", "cursor", "claude_code"] {
            let tier = tier_for(id);
            if active_tools(id).contains(&"veng_run") {
                assert!(
                    !tier.label().contains("no shell distill"),
                    "{id} advertises veng_run and its label denies distilling: {:?}",
                    tier.label()
                );
            }
        }
    }

    /// The reporting tools stay out of `MEMORY` on purpose, and this is what
    /// stops that decision drifting into "add them, they seem useful". They cost
    /// 803 B in the prefix of every request and this file's own rule is that a
    /// tool added without a measurement has no price behind it.
    #[test]
    fn the_mcp_only_tier_gains_the_performer_and_not_the_reporters() {
        let tools = active_tools("antigravity");
        for reporter in [
            "veng_explain_savings",
            "veng_find_noise",
            "veng_context_breakdown",
            "veng_history",
        ] {
            assert!(
                !tools.contains(&reporter),
                "{reporter} was added to the MCP-only set without a measurement"
            );
        }
    }

    /// `strategy.md` section 5 makes this product law: on a Handoff-first host
    /// the built-in tool output is never rewritten, so `veng_run` is the only
    /// path by which the model reads less. Cutting it there would remove the
    /// host's entire reason to have VENG installed.
    #[test]
    fn a_handoff_first_host_keeps_veng_run() {
        assert!(active_tools("cursor").contains(&"veng_run"));
        // And the Full tier is where it stops being worth its 417 bytes, because
        // there the built-in shell is hooked and `veng exec` is one command away.
        assert!(!active_tools("claude_code").contains(&"veng_run"));
    }

    /// An MCP-only host has no shell distillation to report on, so the read
    /// tools over it are noise there. `cline` is registered and takes the
    /// trait's default tier, which is what makes it MCP-only; an id nobody
    /// registered gets `Full` instead.
    #[test]
    fn an_mcp_only_host_gets_the_memory_set_and_nothing_else() {
        let tools = active_tools("cline");
        assert!(tools.contains(&"veng_remember"));
        assert!(tools.contains(&"veng_retrieve"));
        assert!(!tools.contains(&"veng_explain_savings"));
    }

    /// Measured 2026-08-23 over 256 sessions: these two carry 138 of the 149
    /// calls in 467 B, and the six that left cost 1,631 B for 11 calls. If this
    /// list grows without `mcp_surface.py` being re-run, the design's own
    /// justification has stopped being true.
    #[test]
    fn the_full_set_is_the_ones_that_pay_for_the_prefix() {
        let mut got = active_tools("claude_code").to_vec();
        got.sort_unstable();
        assert_eq!(got, vec!["veng_explain_savings", "veng_retrieve"]);
    }

    /// The escape hatch, driven through the predicate instead of the process
    /// environment for the reason in `value_means_all`'s own comment. These are
    /// the values a user actually types.
    #[test]
    fn only_the_documented_spelling_restores_every_tool() {
        assert!(value_means_all(Some("all")));
        assert!(value_means_all(Some("ALL")));
        assert!(!value_means_all(None), "unset must not restore");
        assert!(!value_means_all(Some("")), "empty must not restore");
        assert!(!value_means_all(Some("full")), "undocumented spelling");
    }
}
