use crate::distillers::Distiller;
use crate::pipeline::{OutputSegment, SignalTier};

/// The cloud tool that produced this output, from the command, never from the
/// content: a shape sniff routed `kubectl exec` into the docker summariser,
/// which denied a real `ls:` error (#112).
pub struct CloudDistiller<'a> {
    pub tool: &'a str,
}

impl Distiller for CloudDistiller<'_> {
    fn distill(
        &self,
        segments: &[OutputSegment],
        input: &str,
        _session: Option<&crate::pipeline::SessionState>,
    ) -> Option<String> {
        match self.tool {
            "docker" | "podman" => {
                if input.contains("CONTAINER ID") {
                    Some(distill_docker_ps(input))
                } else if input.contains("Step ") && input.contains(" : ") {
                    Some(distill_docker_build(input))
                } else if is_docker_logs(input) {
                    Some(distill_docker_logs(segments, input))
                } else {
                    // Not a ps table, a build or a log stream: hand it back. A `podman run`
                    // failure once came back as `docker logs: 1 errors/warnings found` (#354).
                    None
                }
            }
            "kubectl" => {
                if is_resource_table(input) {
                    // Any columnar listing, pod tables included: the rows are the answer (#562).
                    // The ledger still folds a listing the agent was already shown.
                    None
                } else {
                    Some(distill_kubectl_generic(segments, input))
                }
            }
            "helm" => Some(distill_helm(input)),
            "terraform" | "tofu" => Some(distill_terraform(input)),
            "aws" => Some(distill_aws(segments, input)),
            // gcloud, az, doctl: no parser, so keep the signal tiers.
            _ => Some(distill_fallback(segments, input)),
        }
    }
}

/// A table with a `NAME` column and an all-caps header; used only to refuse.
fn is_resource_table(input: &str) -> bool {
    input.lines().take(5).any(|l| {
        let cols: Vec<&str> = l.split_whitespace().collect();
        cols.len() >= 3
            && cols.contains(&"NAME")
            && cols.iter().all(|c| {
                c.chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch == '-' || ch == '_')
            })
    })
}

fn is_helm_table(input: &str) -> bool {
    input
        .lines()
        .any(|l| l.contains("REVISION") && l.contains("CHART") && l.contains("STATUS"))
}

fn is_docker_logs(input: &str) -> bool {
    let ts_count = input
        .lines()
        .take(20)
        .filter(|l| {
            l.len() > 20
                && (l
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                    || l.starts_with('['))
        })
        .count();
    ts_count >= 5
}

const CRITICAL_PATTERNS: &[&str] = &[
    "error",
    "Error",
    "ERROR",
    "failed",
    "FAILED",
    "CrashLoopBackOff",
    "OOMKilled",
    "ImagePullBackOff",
    "Terminating",
    "Evicted",
    "panic",
    "fatal",
    "FATAL",
    "exception",
    "Exception",
    "BackOff",
];

const NOISE_PATTERNS: &[&str] = &[
    "Pulling from",
    "Pull complete",
    "Extracting",
    "Waiting",
    "Polling",
    ".......",
    "Downloading",
    "Verifying Checksum",
    "Download complete",
    "Already exists",
    "Using cache",
    " --->",
    " ---> ",
];

fn is_critical(line: &str) -> bool {
    CRITICAL_PATTERNS.iter().any(|p| line.contains(p))
}

fn is_noise(line: &str) -> bool {
    NOISE_PATTERNS.iter().any(|p| line.contains(p))
}

fn distill_kubectl_generic(segments: &[OutputSegment], input: &str) -> String {
    let mut critical_lines: Vec<&str> = Vec::new();
    let mut important_lines: Vec<&str> = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_critical(trimmed) {
            critical_lines.push(trimmed);
        } else if trimmed.contains("configured")
            || trimmed.contains("created")
            || trimmed.contains("unchanged")
            || trimmed.contains("deleted")
        {
            important_lines.push(trimmed);
        }
    }

    let mut out = String::new();
    if !critical_lines.is_empty() {
        for line in critical_lines.iter().take(10) {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !important_lines.is_empty() {
        for line in important_lines.iter().take(10) {
            out.push_str(line);
            out.push('\n');
        }
    }

    if out.trim().is_empty() {
        return distill_fallback(segments, input);
    }

    out.trim().to_string()
}

fn distill_docker_ps(input: &str) -> String {
    let mut running = 0u32;
    let mut exited = 0u32;
    let mut other = 0u32;
    let mut total = 0u32;
    let mut problem_containers: Vec<String> = Vec::new();

    for line in input.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        total += 1;

        // STATUS is column 4 of `CONTAINER ID | IMAGE | COMMAND | CREATED | STATUS | PORTS |
        // NAMES`.
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let status_str = trimmed.to_lowercase();

        if status_str.contains("up ") {
            running += 1;
        } else if status_str.contains("exited") {
            exited += 1;
            if let Some(name) = parts.last() {
                problem_containers.push(name.to_string());
            }
        } else {
            other += 1;
        }
    }

    let mut out = format!(
        "docker: {} containers | {} running, {} exited",
        total, running, exited
    );
    if other > 0 {
        out.push_str(&format!(", {} other", other));
    }
    if !problem_containers.is_empty() {
        out.push_str("\nExited: ");
        let shown: Vec<&str> = problem_containers
            .iter()
            .take(5)
            .map(|s| s.as_str())
            .collect();
        out.push_str(&shown.join(", "));
        if problem_containers.len() > 5 {
            out.push_str(&format!(" +{} more", problem_containers.len() - 5));
        }
    }

    out
}

fn distill_docker_build(input: &str) -> String {
    let mut steps_total = 0u32;
    let mut cached = 0u32;
    let mut error_step: Option<(u32, String)> = None;
    let mut success = false;

    for line in input.lines() {
        let l_lower = line.to_lowercase();
        if l_lower.contains("step ") {
            steps_total += 1;
        }
        if line.contains("Using cache") {
            cached += 1;
        }
        if line.contains("Successfully built") || line.contains("Successfully tagged") {
            success = true;
        }
        if is_critical(line) && !line.contains("Successfully") {
            error_step = Some((steps_total, line.trim().to_string()));
        }
    }

    if let Some((step, msg)) = error_step {
        format!(
            "docker build: ✗ failed at step {}/{}, {}",
            step, steps_total, msg
        )
    } else if success {
        let cached_info = if cached > 0 {
            format!(", {} cached", cached)
        } else {
            String::new()
        };
        format!(
            "docker build: ✓ complete ({} layers{})",
            steps_total, cached_info
        )
    } else {
        format!("docker build: {} steps, {} cached", steps_total, cached)
    }
}

fn distill_docker_logs(_segments: &[OutputSegment], input: &str) -> String {
    let mut critical_lines: Vec<&str> = Vec::new();

    for line in input.lines() {
        if is_critical(line) && !is_noise(line) {
            critical_lines.push(line.trim());
        }
    }

    if critical_lines.is_empty() {
        let total = input.lines().count();
        // "No errors" only over a real log shape: the dispatch can reach here on a weak sniff
        // (#143, #112).
        return super::require_parsed(
            is_docker_logs(input),
            input,
            format!("docker logs: {} lines, no errors detected", total),
        );
    }

    let mut out = format!(
        "docker logs: {} errors/warnings found\n",
        critical_lines.len()
    );
    for line in critical_lines.iter().take(10) {
        out.push_str(line);
        out.push('\n');
    }
    if critical_lines.len() > 10 {
        out.push_str(&format!("... +{} more\n", critical_lines.len() - 10));
    }

    out.trim().to_string()
}

fn distill_terraform(input: &str) -> String {
    let mut added = 0u32;
    let mut changed = 0u32;
    let mut destroyed = 0u32;
    let mut resources: Vec<String> = Vec::new();
    // Whether a plan was read: `init` and `fmt` reach here too.
    let mut parsed = false;

    for line in input.lines() {
        let trimmed = line.trim();

        // A plan line is the anchored address comment only; error prose containing
        // "will be created" read as a clean plan (#633).
        let plan_line = trimmed.strip_prefix("# ").unwrap_or("");

        if plan_line.contains("will be created") {
            parsed = true;
            added += 1;
            if let Some(res) = extract_tf_resource(trimmed) {
                resources.push(format!("+ {}", res));
            }
        } else if plan_line.contains("will be updated") || plan_line.contains("must be replaced") {
            parsed = true;
            changed += 1;
            if let Some(res) = extract_tf_resource(trimmed) {
                resources.push(format!("~ {}", res));
            }
        } else if plan_line.contains("will be destroyed") {
            parsed = true;
            destroyed += 1;
            if let Some(res) = extract_tf_resource(trimmed) {
                resources.push(format!("- {}", res));
            }
        }

        let t_lower = trimmed.to_lowercase();

        // The anchored `Plan:` summary line.
        if t_lower.starts_with("plan:") {
            parsed = true;
            for part in trimmed.split(',') {
                let part = part.trim();
                if part.contains("to add") {
                    if let Some(n) = part.split_whitespace().find_map(|w| w.parse::<u32>().ok())
                        && added == 0
                    {
                        added = n;
                    }
                } else if part.contains("to change") {
                    if let Some(n) = part.split_whitespace().find_map(|w| w.parse::<u32>().ok())
                        && changed == 0
                    {
                        changed = n;
                    }
                } else if part.contains("to destroy")
                    && let Some(n) = part.split_whitespace().find_map(|w| w.parse::<u32>().ok())
                    && destroyed == 0
                {
                    destroyed = n;
                }
            }
        }

        if t_lower.starts_with("apply complete!") {
            return format!(
                "terraform: apply complete +{} ~{} -{}",
                added, changed, destroyed
            );
        }
    }

    let mut out = format!(
        "terraform: +{} ~{} -{} resources",
        added, changed, destroyed
    );

    if !resources.is_empty() {
        out.push('\n');
        for res in resources.iter().take(5) {
            out.push_str(res);
            out.push('\n');
        }
        if resources.len() > 5 {
            out.push_str(&format!("... +{} more\n", resources.len() - 5));
        }
    }

    super::require_parsed(parsed, input, out.trim().to_string())
}

fn extract_tf_resource(line: &str) -> Option<String> {
    // "# aws_instance.web will be created" → "aws_instance.web"
    let trimmed = line.trim().trim_start_matches('#').trim();
    trimmed.split_whitespace().next().map(|s| s.to_string())
}

fn distill_helm(input: &str) -> String {
    let mut deployed = 0u32;
    let mut failed_h = 0u32;
    let mut pending_h = 0u32;
    let mut other_h = 0u32;
    let mut releases: Vec<String> = Vec::new();

    // `REVISION` + `CHART` is what only `helm list` prints.
    let has_header = is_helm_table(input);

    if has_header {
        for line in input.lines().skip(1) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 {
                let name = parts[0];
                // STATUS is usually column 4.
                let status = if parts.len() >= 5 { parts[4] } else { parts[3] };
                match status.to_lowercase().as_str() {
                    "deployed" => deployed += 1,
                    "failed" => {
                        failed_h += 1;
                        releases.push(format!("{} (failed)", name));
                    }
                    "pending-install" | "pending-upgrade" => {
                        pending_h += 1;
                        releases.push(format!("{} ({})", name, status));
                    }
                    _ => other_h += 1,
                }
            }
        }
    }

    let mut out = format!(
        "helm: {} deployed, {} failed, {} pending",
        deployed, failed_h, pending_h
    );
    if other_h > 0 {
        out.push_str(&format!(", {} other", other_h));
    }
    if !releases.is_empty() {
        out.push_str("\nIssues: ");
        out.push_str(&releases.join(", "));
    }

    // No release table: no count to assert.
    super::require_parsed(has_header, input, out)
}

fn distill_aws(segments: &[OutputSegment], input: &str) -> String {
    let mut critical_lines: Vec<&str> = Vec::new();
    let mut result_lines: Vec<&str> = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_critical(trimmed) {
            critical_lines.push(trimmed);
        } else if !is_noise(trimmed)
            && (trimmed.contains("arn:")
                || trimmed.contains("i-")
                || trimmed.contains("sg-")
                || trimmed.contains("vpc-")
                || trimmed.contains("subnet-")
                || trimmed.contains("Successfully")
                || trimmed.contains("completed"))
        {
            result_lines.push(trimmed);
        }
    }

    let mut out = String::new();

    if !critical_lines.is_empty() {
        for line in critical_lines.iter().take(5) {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !result_lines.is_empty() {
        for line in result_lines.iter().take(10) {
            out.push_str(line);
            out.push('\n');
        }
    }

    if out.trim().is_empty() {
        return distill_fallback(segments, input);
    }

    out.trim().to_string()
}

fn distill_fallback(segments: &[OutputSegment], input: &str) -> String {
    let mut out = String::new();
    let mut lines = 0;

    for seg in segments {
        if matches!(seg.tier, SignalTier::Critical | SignalTier::Important) {
            for line in seg.content.lines() {
                if lines >= 20 {
                    break;
                }
                out.push_str(line);
                out.push('\n');
                lines += 1;
            }
        }
        if lines >= 20 {
            break;
        }
    }

    if out.trim().is_empty() {
        for seg in segments.iter().take(10) {
            out.push_str(&seg.content);
            out.push('\n');
        }
    }

    // Never return empty, which reads as a 100% saving.
    super::require_parsed(!out.trim().is_empty(), input, out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `podman run` failure is not summarised as docker logs (#354).
    #[test]
    fn hands_back_a_podman_failure_instead_of_calling_it_docker_logs() {
        let input = "Cannot connect to Podman. Please verify your connection to the Linux system using `podman system connection list`, or try `podman machine init` and `podman machine start` to manage a new Linux VM\nError: unable to connect to Podman socket: dial unix /tmp/podman.sock: connect: no such file or directory\n";

        let out = CloudDistiller { tool: "podman" }.distill(&[], input, None);

        assert_eq!(
            out, None,
            "a payload this tool cannot identify must be handed back, got: {out:?}"
        );
    }

    /// A real log stream is still summarised.
    #[test]
    fn still_summarises_a_real_docker_log_stream() {
        let mut input = String::new();
        for i in 0..12 {
            input.push_str(&format!(
                "2026-08-07T10:{i:02}:00.000000000Z worker ready and serving requests\n"
            ));
        }
        let out = CloudDistiller { tool: "docker" }.distill(&[], &input, None);
        assert!(out.is_some(), "a timestamped stream is docker logs");
    }

    /// Healthy `Bound` PVCs are not errors (#105).
    const PVC_TABLE: &str = "\
NAMESPACE    NAME                      STATUS   VOLUME         CAPACITY   ACCESS MODES   STORAGECLASS   AGE
datasource   qdrant-storage-qdrant-0   Bound    pvc-0a75d858   5Gi        RWO            default        2y
consul       data-consul-0             Bound    pvc-1b86e969   10Gi       RWO            default        2y";

    const PODS_ALL_NAMESPACES: &str = "\
NAMESPACE   NAME                     READY   STATUS             RESTARTS   AGE
default     api-7fb96c846b-f4jnm     1/1     Running            0          5d
kube-系统   auth-5d4f6c8b99-abc12    0/1     CrashLoopBackOff   15         2h";

    fn distill_as(tool: &str, input: &str) -> String {
        CloudDistiller { tool }
            .distill(&[], input, None)
            .unwrap_or_else(|| input.to_string())
    }

    fn distill(input: &str) -> String {
        distill_as("kubectl", input)
    }

    /// `kubectl exec` output is not docker logs (#112).
    #[test]
    fn routes_by_command_not_by_content_shape() {
        let mut input = String::from("ls: /data/models: No such file or directory\n");
        for i in 0..8 {
            input.push_str(&format!(
                "2026-07-20T10:0{i}:11Z  worker heartbeat ok, queue depth 0, uptime 42h\n"
            ));
        }

        assert!(
            is_docker_logs(&input),
            "precondition: the sniff still fires"
        );
        assert_eq!(distill_as("kubectl", &input), input);
    }

    #[test]
    fn refuses_a_release_summary_without_a_helm_table() {
        let input = "Release \"api\" has been upgraded. Happy Helming!\nNAMESPACE: prod\n";
        assert_eq!(distill_as("helm", input), input);
    }

    #[test]
    fn refuses_a_plan_summary_when_no_plan_was_read() {
        let input =
            "Terraform has been successfully initialized!\n\nTry running \"terraform plan\".\n";
        assert_eq!(distill_as("terraform", input), input);
    }

    /// A failing `terraform validate` is not a clean plan, wrapped as terraform wraps it (#633).
    #[test]
    fn refuses_to_count_resources_out_of_terraform_error_prose() {
        let input = "function works only with files that are distributed as part of the\n\
                     configuration source code, so if this file will be created by a resource in\n\
                     this configuration you must instead obtain this result from an attribute of\n\
                     that resource.\n";
        assert_eq!(distill_as("terraform", input), input);
    }

    #[test]
    fn returns_input_rather_than_nothing_when_no_segment_survives() {
        let input = "some output no cloud parser understands\n";
        assert_eq!(distill_as("gcloud", input), input);
    }

    #[test]
    fn passes_non_pod_resource_tables_through_untouched() {
        assert_eq!(distill(PVC_TABLE), PVC_TABLE);
    }

    #[test]
    fn keeps_every_row_of_a_pod_table() {
        // Pod rows are the answer (#562).
        assert_eq!(distill(PODS_ALL_NAMESPACES), PODS_ALL_NAMESPACES);
    }

    #[test]
    fn returns_input_when_pod_table_has_no_rows() {
        let input = "NAME   READY   STATUS   RESTARTS   AGE\n";
        assert_eq!(distill(input), input);
    }
}
