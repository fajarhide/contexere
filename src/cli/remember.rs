use crate::store::sqlite::Store;
use anyhow::Result;
use std::sync::Arc;

/// Read by `super::check_flags`, the same list every other subcommand keeps.
const FLAGS: super::Flags = &[
    (
        "-c, --category <name>",
        "decision, pattern, gotcha, or fact (default: fact)",
    ),
    ("-t, --tags <a,b>", "Comma-separated tags"),
    ("--global", "Store outside the current project's scope"),
    ("--project-scoped", "Scope to this project (the default)"),
];

/// The parsed form of `omni remember`, hand-read from argv.
///
/// This was the one subcommand of seventeen that used clap's result rather than
/// discarding it and re-parsing (#506). Four fields is less code than the
/// dependency, and `check_flags` already rejects an unknown flag for every other
/// subcommand, so this one now fails the same way rather than through clap's
/// exit path.
struct RememberArgs {
    content: String,
    category: String,
    tags: Option<String>,
    project_scoped: bool,
}

fn parse(args: &[String]) -> Result<RememberArgs> {
    super::check_flags("remember", args, FLAGS)?;

    let mut content: Option<String> = None;
    let mut category = "fact".to_string();
    let mut tags = None;
    let mut project_scoped = true;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        // `--flag=value` and `--flag value` both, because the old parser took
        // both and a user who learned one should not find it gone.
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        let mut value = || -> Result<String> {
            match inline.clone() {
                Some(v) => Ok(v),
                None => it
                    .next()
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("`{name}` needs a value")),
            }
        };
        match name {
            "-c" | "--category" => category = value()?,
            "-t" | "--tags" => tags = Some(value()?),
            "--global" => project_scoped = false,
            // Accepted because it always was, and it is the default. Under clap
            // it was `default_value_t = true` on a plain bool, so passing it
            // could not change anything and there was no way to ask for global
            // scope at all, against a doc comment that said global was the
            // default. `--global` is the half that was missing.
            "--project-scoped" => project_scoped = true,
            _ if content.is_none() => content = Some(arg.clone()),
            _ => anyhow::bail!("unexpected argument `{arg}`, quote the whole memory as one string"),
        }
    }

    Ok(RememberArgs {
        content: content
            .ok_or_else(|| anyhow::anyhow!("nothing to remember: omni remember \"<text>\""))?,
        category,
        tags,
        project_scoped,
    })
}

pub fn run(args: &[String], store: Arc<Store>) -> Result<()> {
    let parsed = parse(args)?;

    if parsed.content.trim().len() < 10 {
        anyhow::bail!("Memory entry more short (min 10 character), write more specific");
    }
    if parsed.content.len() > 2000 {
        anyhow::bail!("Memory entry more long (max 2000 character), write more specific");
    }
    let valid_categories = ["decision", "pattern", "gotcha", "fact"];
    if !valid_categories.contains(&parsed.category.as_str()) {
        anyhow::bail!("Not valid categories. Try: decision, pattern, gotcha, fact");
    }

    let project_path = if parsed.project_scoped {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "global".to_string())
    } else {
        "global".to_string()
    };

    let project_hash = crate::agents::multiagent::project_hash(&project_path);

    let tags: Vec<String> = parsed
        .tags
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let prefix_len = 20.min(parsed.content.len());
    let key = format!(
        "[{}] {}",
        parsed.category,
        crate::util::text::safe_slice(&parsed.content, prefix_len)
    );

    store.upsert_project_knowledge(
        &project_hash,
        &key,
        &parsed.content,
        0.9, // high confidence, user explicitly set this
    );

    let display_len = 60.min(parsed.content.len());
    println!(
        "✓ Saved as [{}]: {}",
        parsed.category,
        crate::util::text::safe_slice(&parsed.content, display_len)
    );
    if !tags.is_empty() {
        println!("  Tags: {}", tags.join(", "));
    }
    println!("  Scope: {}", project_path);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_value_attached_or_separate() {
        let split = parse(&["a memory".into(), "-c".into(), "gotcha".into()]).expect("parses");
        let joined = parse(&["a memory".into(), "--category=gotcha".into()]).expect("parses");

        assert_eq!(split.category, "gotcha");
        assert_eq!(joined.category, "gotcha");
        assert_eq!(split.content, "a memory");
    }

    /// #151's class: a flag nobody parsed used to run the default and exit 0.
    #[test]
    fn rejects_a_flag_it_does_not_know() {
        assert!(parse(&["a memory".into(), "--catgory".into(), "fact".into()]).is_err());
    }

    /// Under clap this could not be expressed: the bool defaulted to true, so
    /// every memory was project-scoped whatever was passed.
    #[test]
    fn global_scope_is_reachable() {
        assert!(
            !parse(&["a memory".into(), "--global".into()])
                .unwrap()
                .project_scoped
        );
        assert!(parse(&["a memory".into()]).unwrap().project_scoped);
    }
}
