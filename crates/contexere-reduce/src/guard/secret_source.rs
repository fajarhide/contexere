//! Decide by where the bytes came from, not by their shape (#981). A bare
//! `<id> <secret>` pair has no shape a redactor can see, but the command that
//! printed it names its source.

/// Programs that put a file's bytes on stdout. Anything else that reads a file
/// (`.`, `wc`, `stat`, `shasum`, `cmp`, `jq --rawfile`) is left alone.
const PRINTERS: &[&str] = &[
    "cat", "head", "tail", "less", "more", "bat", "tac", "nl", "sed", "awk", "gawk", "grep",
    "egrep", "rg", "ag", "strings", "xxd", "od", "hexdump", "base64", "cut", "sort", "uniq",
    "column", "paste", "diff", "jq",
];
/// A last pipeline stage that reports only a count or a digest.
const SINKS: &[&str] = &[
    "wc",
    "shasum",
    "sha256sum",
    "sha1sum",
    "md5",
    "md5sum",
    "cmp",
];
/// Words that run the next word as the command.
const WRAPPERS: &[&str] = &[
    "sudo",
    "env",
    "time",
    "nice",
    "exec",
    "command",
    "nohup",
    "omni",
    "contexere",
    "eval",
    "{",
    "!",
    "if",
    "elif",
    "else",
    "then",
    "do",
    "while",
    "until",
];
/// The first positional of these is a pattern or a script, not a file.
const TAKES_A_PATTERN: &[&str] = &["sed", "awk", "gawk", "grep", "egrep", "rg", "ag", "jq"];
/// Programs that take a script on the command line, which may open a file.
const INTERPRETERS: &[&str] = &["python", "python3", "perl", "ruby", "node", "php"];
/// Files that hold credentials whatever directory they sit in.
const SOURCE_NAMES: &[&str] = &[
    ".netrc",
    "_netrc",
    ".pgpass",
    ".npmrc",
    ".pypirc",
    ".git-credentials",
    ".htpasswd",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
];
/// A file named `secret` with one of these is about secrets, not one.
const NOT_A_CREDENTIAL_FILE: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "mjs", "go", "rb", "java", "kt", "c", "h", "cpp", "md",
    "mdx", "html", "css", "sh", "tf", "sql", "lock", "snap", "log", "png", "svg",
];
const SAMPLE_ENV_WORDS: &[&str] = &["example", "sample", "template", "dist", "test"];

fn program(word: &str) -> &str {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
    name.strip_suffix(".exe").unwrap_or(name)
}

fn trimmed(word: &str) -> &str {
    word.trim_matches(|c: char| "'\"`();,\\".contains(c))
        .trim_start_matches('<')
}

/// Whether a path names a credential source. `extra` is the user's list, matched
/// as a path suffix.
pub fn is_source(path: &str, extra: &[String]) -> bool {
    let path = path.replace('\\', "/");
    let path = path.trim_end_matches('/');
    let Some(name) = path.rsplit('/').next().filter(|n| !n.is_empty()) else {
        return false;
    };
    if name.starts_with('-') || name.contains(['=', '$', '*']) {
        return false;
    }
    if extra
        .iter()
        .any(|e| !e.is_empty() && path.ends_with(e.trim_start_matches("~/")))
    {
        return true;
    }
    let name = name.to_lowercase();
    if path.ends_with(".aws/credentials")
        || path.ends_with("gh/hosts.yml")
        || path.ends_with(".docker/config.json")
        || path.ends_with(".kube/config")
        || SOURCE_NAMES.contains(&name.as_str())
        || name.ends_with(".tfvars")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || (name.starts_with("service-account") && name.ends_with(".json"))
    {
        return true;
    }
    // A certificate is public, and it shares the extension with its key.
    if (name.ends_with(".pem") || name.ends_with(".key"))
        && !["cert", "chain", "pub", "ca."]
            .iter()
            .any(|w| name.contains(w))
    {
        return true;
    }
    if path.contains(".ssh/id_") {
        return !name.ends_with(".pub");
    }
    if name == ".env" || name == ".envrc" {
        return true;
    }
    if let Some(rest) = name.strip_prefix(".env.") {
        return !SAMPLE_ENV_WORDS.iter().any(|w| rest.contains(w));
    }
    let extension = name.rsplit_once('.').map_or("", |(_, e)| e);
    ["secret", "credential", "creds"]
        .iter()
        .any(|w| name.contains(w))
        && !NOT_A_CREDENTIAL_FILE.contains(&extension)
}

/// The user's own sources, from `OMNI_SECRET_SOURCES`, colon separated.
pub fn user_sources() -> Vec<String> {
    std::env::var("OMNI_SECRET_SOURCES")
        .map(|v| v.split(':').map(str::to_string).collect())
        .unwrap_or_default()
}

/// Commands whose whole output can hold credentials, wherever they run. Takes one
/// pipeline stage, so a later `cut -f 1` is not read as docker's format flag.
fn remote_source(words: &[&str]) -> Option<&'static str> {
    let has = |a: &str, b: &str| words.windows(2).any(|w| program(w[0]) == a && w[1] == b);
    if has("crontab", "-l") {
        return Some("crontab -l");
    }
    if has("systemctl", "cat") {
        return Some("systemctl cat");
    }
    let formatted = words
        .iter()
        .any(|w| *w == "-f" || w.starts_with("--format") || w.starts_with("-f="));
    let inspects = has("docker", "inspect")
        || words
            .windows(3)
            .any(|w| program(w[0]) == "docker" && w[1] == "container" && w[2] == "inspect");
    (inspects && !formatted).then_some("docker inspect")
}

/// A `grep -c`, `-l` or `-q` reports about the file without printing its lines,
/// and `sed -i` writes it back. Options are walked in order: `-e` and `-f` take the
/// next word, so a pattern such as `-client` is never read as flags, and `--` ends them.
fn prints_nothing_of_the_file(name: &str, args: &[&str]) -> bool {
    let (letters, long): (&str, &[&str]) = match name {
        "grep" | "egrep" | "rg" | "ag" => (
            "clqL",
            &["--count", "--quiet", "--files-with-matches", "--silent"],
        ),
        "sed" => ("i", &["--in-place"]),
        // These read the file into a variable; the filter decides what prints.
        "jq" => {
            return args
                .iter()
                .any(|w| *w == "--rawfile" || *w == "--slurpfile");
        }
        _ => return false,
    };
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if *word == "--" {
            return false;
        }
        if long.contains(word) {
            return true;
        }
        if !word.starts_with('-') || word.starts_with("--") {
            continue;
        }
        for c in word.chars().skip(1) {
            if letters.contains(c) {
                return true;
            }
            if matches!(c, 'e' | 'f') {
                words.next();
                break;
            }
        }
    }
    false
}

/// `>`, `>>`, `2>&1`, or a target attached as in `>file`.
fn is_redirection(word: &str) -> bool {
    word.trim_start_matches(|c: char| c.is_ascii_digit())
        .starts_with('>')
}

/// What a command would print from a credential source, or `None`. Ignores quoting,
/// so `ssh host 'cat ~/.env'` and `bash -c` are seen. The known gap is a pipe inside a
/// quoted pattern, `grep 'a | b' .env`, which splits the stage. A hook cannot stop a
/// command built to evade it; this catches the ones written in good faith.
pub fn printed_source(command: &str, extra: &[String]) -> Option<String> {
    let command = spaced_input_redirects(&command.replace("\\\n", " "));
    let spaced = command.replace('|', " | ");
    let spaced = spaced.replace("&&", "\n").replace(';', "\n");
    let spaced = expand_source_variables(spaced, extra);
    for statement in spaced.lines() {
        let raw: Vec<&str> = statement.split_whitespace().collect();
        let stages: Vec<&[&str]> = raw.split(|w| *w == "|").collect();
        let ends_in_a_sink = stages.len() > 1
            && stages
                .last()
                .and_then(|s| s.first())
                .is_some_and(|w| SINKS.contains(&program(trimmed(w))));
        if ends_in_a_sink {
            continue;
        }
        for stage in stages {
            let words: Vec<&str> = stage.iter().map(|w| trimmed(w)).collect();
            if let Some(found) = remote_source(&words) {
                return Some(found.to_string());
            }
            if let Some(found) = other_reader(stage, &words, extra) {
                return Some(found);
            }
            let Some(at) = printer_position(stage) else {
                continue;
            };
            let name = program(trimmed(stage[at]));
            let args = &stage[at + 1..];
            let plain: Vec<&str> = args.iter().map(|w| trimmed(w)).collect();
            if prints_nothing_of_the_file(name, &plain) {
                continue;
            }
            if let Some(found) = source_argument(name, args, extra) {
                return Some(found.to_string());
            }
        }
    }
    None
}

/// `cat<file` as `cat <file`. A heredoc's `<<` and process substitution stay whole.
fn spaced_input_redirects(command: &str) -> String {
    let chars: Vec<char> = command.chars().collect();
    let mut out = String::with_capacity(command.len() + 4);
    for (i, c) in chars.iter().enumerate() {
        let single = *c == '<'
            && (i == 0 || chars[i - 1] != '<')
            && !matches!(chars.get(i + 1), Some('<' | '('));
        if single {
            out.push(' ');
        }
        out.push(*c);
    }
    out
}

/// `f=.env; cat $f` and `for f in .env; do cat $f; done`, with the variable written
/// out, so the printer sees the file. Only a value naming a source is substituted.
fn expand_source_variables(mut text: String, extra: &[String]) -> String {
    let words: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    let mut found: Vec<(String, String)> = Vec::new();
    for (i, word) in words.iter().enumerate() {
        if word == "for" && words.get(i + 2).is_some_and(|w| w == "in") {
            if let (Some(name), Some(value)) = (words.get(i + 1), words.get(i + 3))
                && is_source(trimmed(value), extra)
            {
                found.push((name.clone(), trimmed(value).to_string()));
            }
        } else if let Some((name, value)) = word.split_once('=')
            && !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && is_source(trimmed(value), extra)
        {
            found.push((name.to_string(), trimmed(value).to_string()));
        }
    }
    for (name, value) in found {
        let braced = format!("${{{name}}}");
        text = text.replace(&braced, &value);
        let plain = format!("${name}");
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(at) = rest.find(&plain) {
            let (before, tail) = rest.split_at(at);
            let after = tail.strip_prefix(plain.as_str()).unwrap_or(tail);
            out.push_str(before);
            if after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
                out.push_str(&plain);
            } else {
                out.push_str(&value);
            }
            rest = after;
        }
        out.push_str(rest);
        text = out;
    }
    text
}

/// Readers that are not plain printers of their arguments. `raw` keeps the quoting
/// split as it came, since an interpreter's script is one logical argument.
fn other_reader(raw: &[&str], words: &[&str], extra: &[String]) -> Option<String> {
    let first = program(words.iter().find(|w| !w.is_empty())?);
    // `tr a-z A-Z <file` and `while read l; do echo $l; done <file`. Any other program
    // reading its stdin from a file is loading it, which is what the denial recommends.
    if PRINTERS.contains(&first) || first == "tr" || first == "done" {
        for (i, word) in raw.iter().enumerate() {
            let target = match *word {
                "<" => raw.get(i + 1).copied(),
                w if w.starts_with('<') && !w.starts_with("<<") => Some(w),
                _ => None,
            };
            if let Some(path) = target.map(trimmed)
                && is_source(path, extra)
            {
                return Some(path.to_string());
            }
        }
    }
    if first == "git"
        && let Some(sub) = words.get(1)
    {
        let shows = matches!(*sub, "show" | "diff" | "blame")
            || (*sub == "log" && words.iter().any(|w| *w == "-p" || *w == "--patch"));
        if shows {
            return words[2..]
                .iter()
                .map(|w| w.rsplit(':').next().unwrap_or(w))
                .find(|w| is_source(w, extra))
                .map(str::to_string);
        }
    }
    if first == "dd" {
        return words
            .iter()
            .filter_map(|w| w.strip_prefix("if="))
            .find(|w| is_source(w, extra))
            .map(str::to_string);
    }
    if first == "cp" && words.iter().any(|w| w.starts_with("/dev/std")) {
        return words[1..]
            .iter()
            .find(|w| is_source(w, extra))
            .map(|w| w.to_string());
    }
    if INTERPRETERS.contains(&first) && words.iter().any(|w| matches!(*w, "-c" | "-e")) {
        return raw
            .join(" ")
            .split(|c: char| !(c.is_ascii_alphanumeric() || "._/~-".contains(c)))
            .find(|w| is_source(w, extra))
            .map(str::to_string);
    }
    None
}

/// Where a printer sits in command position, so `gh pr create --body "cat the secret"`
/// is prose: first in the stage after any wrapper and assignment, or the command of
/// `ssh`, `sh -c` or `kubectl exec --`. `TOKEN=$(cat file) ./run` is not here: it loads
/// the value without printing it.
fn printer_position(stage: &[&str]) -> Option<usize> {
    let first_is_ssh = stage.first().is_some_and(|w| program(w) == "ssh");
    // `sudo -u app cat x` and `env -i cat x`: a wrapper's options, and a value after one.
    let wrapped = |before: &[&str]| {
        before.iter().enumerate().all(|(k, w)| {
            WRAPPERS.contains(&program(w))
                || (w.contains('=') && !w.starts_with('-'))
                || w.starts_with('-')
                || (k > 0 && before[k - 1].starts_with('-'))
        })
    };
    stage.iter().enumerate().find_map(|(i, word)| {
        if !PRINTERS.contains(&program(trimmed(word))) {
            return None;
        }
        let after_wrappers = wrapped(&stage[..i]);
        let handed_over = i > 0 && matches!(stage[i - 1], "--" | "-c" | "-lc");
        (after_wrappers || handed_over || first_is_ssh).then_some(i)
    })
}

/// The first argument of a printer that names a source. A quoted pattern is skipped
/// whole, since whitespace split it, and a redirection's target is written, not printed.
fn source_argument<'a>(name: &str, args: &[&'a str], extra: &[String]) -> Option<&'a str> {
    let plain = |w: &&&str| trimmed(w) == "-e" || trimmed(w) == "-f";
    let mut skip_pattern = TAKES_A_PATTERN.contains(&name) && !args.iter().any(|w| plain(&w));
    let mut skip_target = false;
    let mut open_quote = None;
    let mut options_ended = false;
    for raw in args.iter().copied() {
        if let Some(q) = open_quote {
            if raw.contains(q) {
                open_quote = None;
            }
            continue;
        }
        let word = trimmed(raw);
        if skip_target {
            skip_target = false;
            continue;
        }
        if is_redirection(word) {
            skip_target = matches!(word, ">" | ">>" | "1>" | "1>>" | "2>" | "2>>");
            continue;
        }
        if word == "--" {
            options_ended = true;
            continue;
        }
        if word.starts_with('-') && !options_ended {
            continue;
        }
        if skip_pattern {
            skip_pattern = false;
            if let Some(q) = raw.chars().next().filter(|c| matches!(c, '\'' | '"'))
                && raw.matches(q).count() == 1
            {
                open_quote = Some(q);
            }
            continue;
        }
        if is_source(word, extra) {
            return Some(word);
        }
    }
    None
}

/// The pre-hook's refusal, naming what to run instead so the agent does not try
/// another filter.
pub fn denial(source: &str) -> String {
    format!(
        "Contexere refused this: it would print {source} into the model's context, and filtering \
         it with sed or cut only changes the shape, not the values. To use the values, load them \
         without printing (`. file`, `jq --rawfile x file | curl -d @-`). To inspect the file, use \
         `wc -l`, `stat`, `shasum`, `cmp` or `grep -c`. Ask the user to read it if you need its contents."
    )
}

/// What a fail-closed reply holds instead of the bytes: their shape only.
pub fn summary(source: &str, content: &str) -> String {
    let split = |l: &str| -> Vec<usize> {
        l.split(|c: char| c.is_whitespace() || matches!(c, ',' | '=' | ':'))
            .filter(|f| !f.is_empty())
            .map(str::len)
            .collect()
    };
    let lines = content.lines().count();
    let widths: Vec<Vec<usize>> = content.lines().map(split).collect();
    let columns = widths.iter().map(Vec::len).max().unwrap_or(0);
    let shown: Vec<String> = widths
        .iter()
        .take(5)
        .map(|w| w.iter().map(usize::to_string).collect::<Vec<_>>().join(" "))
        .collect();
    format!(
        "[OMNI: output of {source} withheld, it is a credential source. {lines} lines, up to \
         {columns} columns, field lengths by line: {}]",
        shown.join(" / ")
    )
}

fn names_a_secret(cell: &str) -> bool {
    let cell = cell
        .trim()
        .trim_matches('"')
        .to_lowercase()
        .replace(['_', '-'], " ");
    [
        "secret",
        "access key",
        "password",
        "passwd",
        "private key",
        "api key",
    ]
    .iter()
    .any(|w| cell.contains(w))
        || cell.split_whitespace().any(|w| w == "token")
}

/// A CSV or TSV whose header names a secret, with those columns hidden on every
/// row, and the count hidden. A row of another width is hidden whole, since its
/// columns cannot be trusted to line up. `None` when there is nothing to hide.
pub fn mask_named_columns(content: &str) -> Option<(String, usize)> {
    let mut lines = content.lines();
    let header = lines.next()?;
    let delimiter = ['\t', ','].into_iter().find(|d| header.contains(*d))?;
    let cells: Vec<&str> = header.split(delimiter).collect();
    let hidden: Vec<usize> = cells
        .iter()
        .enumerate()
        .filter(|(_, c)| names_a_secret(c))
        .map(|(i, _)| i)
        .collect();
    if hidden.is_empty() || cells.len() < 2 {
        return None;
    }
    // A quote can hold the delimiter, so a split cannot be trusted to line up. Such a
    // record is withheld rather than parsed.
    let header_quoted = header.contains('"');
    let mut out = vec![header.to_string()];
    let mut count = 0;
    for line in lines {
        if line.trim().is_empty() {
            out.push(line.to_string());
            continue;
        }
        let row: Vec<&str> = line.split(delimiter).collect();
        if header_quoted || line.contains('"') || row.len() != cells.len() {
            count += 1;
            out.push("[REDACTED]".to_string());
            continue;
        }
        let kept: Vec<&str> = row
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if hidden.contains(&i) {
                    count += 1;
                    "[REDACTED]"
                } else {
                    c
                }
            })
            .collect();
        out.push(kept.join(&delimiter.to_string()));
    }
    (count > 0).then(|| (out.join("\n") + "\n", count))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shapes written in good faith that printed a source without being seen
    /// (GHSA-v3fc-v69m-9v8h).
    #[test]
    fn sees_the_shell_shapes_and_files_it_used_to_miss() {
        for command in [
            "cat<.env",
            "{ cat .env; }",
            "if true; then cat .env; fi",
            "for f in .env; do cat $f; done",
            "f=.env; cat $f",
            "f=.env; cat \"${f}\"",
            "while read l; do echo $l; done < .env",
            "tr a-z A-Z < .env",
            "dd if=.env",
            "diff .env /dev/null",
            "cp .env /dev/stdout",
            "git show HEAD:.env",
            "git log -p -- .env",
            "git diff .env",
            "jq . .env",
            "eval \"cat .env\"",
            "python3 -c 'print(open(\".env\").read())'",
            "cat ~/.netrc",
            "cat ~/.pgpass",
            "cat ~/.npmrc",
            "cat ~/.docker/config.json",
            "cat ~/.kube/config",
            "cat id_rsa",
            "cat deploy/key.pem",
            "cat prod.tfvars",
            "cat service-account.json",
            "cat .git-credentials",
        ] {
            assert!(printed(command), "{command}");
        }
    }

    /// Reading without printing is what the denial tells the agent to do.
    #[test]
    fn leaves_alone_what_uses_a_source_without_printing_it() {
        for command in [
            "wc -l < .env",
            ". .env",
            "gh auth login --with-token < .npmrc",
            "jq --rawfile x .env -n '$x | length'",
            "git add .env",
            "git log -- .env",
            "git status",
            "cat cert.pem",
            "cat fullchain.pem",
            "cat id_rsa.pub",
            "cat <<EOF",
            "python3 -c 'print(1)'",
            "cp .env .env.bak",
            "f=main.rs; cat $f",
            "for f in a.rs b.rs; do cat $f; done",
        ] {
            assert!(!printed(command), "{command}");
        }
    }

    fn printed(command: &str) -> bool {
        printed_source(command, &[]).is_some()
    }

    #[test]
    fn refuses_a_plain_read_of_each_default_source() {
        for c in [
            "cat ~/.aws/credentials",
            "cat ~/.config/gh/hosts.yml",
            "cat .env",
            "head -3 .env.local",
            "cat ~/.storage-creds",
            "cat creds.csv",
            "tail ~/.ssh/id_ed25519",
            "cat k8s/secrets.yaml",
        ] {
            assert!(printed(c), "{c}");
        }
    }

    #[test]
    fn refuses_a_filter_that_only_changes_the_shape() {
        assert!(printed("sed 's/=.*/=x/' .env"));
        assert!(printed("cat .env | sed 's/=/ /'"));
        assert!(printed("grep -e TOKEN .env"));
        assert!(printed("sort < .env"));
    }

    #[test]
    fn refuses_inside_ssh_and_a_shell() {
        assert!(printed("ssh host 'cat ~/.env'"));
        assert!(printed("bash -c \"cat .env\""));
    }

    #[test]
    fn refuses_past_a_wrapper_or_a_passthrough_prefix() {
        assert!(printed("OMNI_PASSTHROUGH=1 cat .env"));
        assert!(printed("omni exec cat .env"));
    }

    #[test]
    fn refuses_the_remote_sources() {
        assert!(printed("crontab -l"));
        assert!(printed("ssh host crontab -l"));
        assert!(printed("systemctl cat app"));
        assert!(printed("docker inspect app"));
    }

    #[test]
    fn refuses_past_a_wrappers_own_options() {
        assert!(printed("sudo -u app cat /app/.env"));
        assert!(printed("env -i cat .env"));
    }

    #[test]
    fn refuses_a_pattern_that_only_looks_like_a_safe_flag() {
        assert!(printed("grep -e '-client' .env"));
        assert!(printed("grep -- -c .env"));
    }

    #[test]
    fn refuses_docker_inspect_when_a_later_stage_has_the_flag() {
        assert!(printed("docker inspect app | cut -f 1"));
    }

    #[test]
    fn allows_counting_a_remote_source() {
        assert!(!printed("crontab -l | wc -l"));
    }

    #[test]
    fn knows_a_windows_path_and_program() {
        assert!(is_source("C:\\project\\.env", &[]));
        assert!(is_source("C:\\Users\\me\\.ssh\\id_ed25519", &[]));
        assert!(printed("cat.exe .env"));
    }

    #[test]
    fn allows_a_formatted_docker_inspect() {
        assert!(!printed("docker inspect -f '{{.State.Status}}' app"));
    }

    #[test]
    fn keeps_every_command_the_issue_requires() {
        for c in [
            ". .env",
            "source .env",
            "jq --rawfile x .env '.' | curl -d @- https://example.com",
            "wc -l .env",
            "stat .env",
            "shasum .env",
            "cmp .env .env.bak",
            "cat .env | wc -l",
            "grep -c TOKEN .env",
            "sed -i 's/a/b/' .env",
        ] {
            assert!(!printed(c), "{c}");
        }
    }

    #[test]
    fn allows_writing_to_a_source() {
        assert!(!printed("cat > .env"));
        assert!(!printed("cat template >> .env"));
        assert!(!printed("cat notes 2>&1"));
    }

    #[test]
    fn allows_prose_and_a_multi_word_pattern_that_mention_a_secret() {
        assert!(!printed("gh pr create --body \"cat the secret file\""));
        assert!(!printed("grep -n \"fn store_credential\" src/vault.rs"));
    }

    #[test]
    fn refuses_a_read_through_a_container_exec() {
        assert!(printed("kubectl exec pod -- cat /app/.env"));
    }

    #[test]
    fn allows_loading_a_value_into_a_variable() {
        assert!(!printed("TOKEN=$(cat ~/.storage-creds) ./run.sh"));
    }

    #[test]
    fn allows_a_later_stage_whose_pattern_names_a_secret() {
        assert!(!printed("cat notes | grep -n secret"));
        assert!(!printed("git log | sed -n 1p | grep credentials"));
    }

    #[test]
    fn allows_a_pattern_or_a_file_that_is_only_about_secrets() {
        for c in [
            "grep -rn secret src/",
            "grep credentials README.md",
            "cat src/secret_store.rs",
            "cat docs/credentials.md",
            "cat .env.example",
            "cat ~/.ssh/id_ed25519.pub",
            "cat Cargo.toml",
        ] {
            assert!(!printed(c), "{c}");
        }
    }

    #[test]
    fn reads_the_users_own_list() {
        let extra = vec!["~/.portal-key".to_string()];
        assert!(printed_source("cat ~/.portal-key", &extra).is_some());
        assert!(printed_source("cat ~/.portal-key", &[]).is_none());
    }

    #[test]
    fn summary_holds_no_fragment_of_the_value() {
        let secret = "wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY01";
        let out = summary(".env", &format!("AKIAIOSFODNN7EXAMPLE {secret}\n"));
        assert!(!out.contains("wJalrXUt"));
        assert!(!out.contains("AKIAIOSF"));
        assert!(out.contains("1 lines") && out.contains("20 40"));
    }

    #[test]
    fn hides_every_secret_column_on_every_row() {
        let csv = "User Name,Access Key Id,Secret Access Key\nalice,AKIAONE,sekretone\nbob,AKIATWO,sekrettwo\n";
        let (out, count) = mask_named_columns(csv).expect("hidden");
        assert_eq!(count, 4);
        assert!(out.contains("alice,[REDACTED],[REDACTED]"));
        assert!(!out.contains("sekret") && !out.contains("AKIA"));
    }

    #[test]
    fn withholds_a_record_a_quote_could_split() {
        let csv = "name,\"password, current\"\nalice,\"prefix,opaque-secret\"\n";
        let (out, _) = mask_named_columns(csv).expect("hidden");
        assert!(!out.contains("opaque-secret"));
    }

    #[test]
    fn hides_a_row_of_another_width_whole() {
        let (out, _) = mask_named_columns("name,token\nalice,x,y\n").expect("hidden");
        assert!(!out.contains("alice"));
    }

    #[test]
    fn leaves_a_table_with_no_secret_column_alone() {
        assert!(mask_named_columns("name,tokens_used\nalice,12\n").is_none());
        assert!(mask_named_columns("just some prose, with a comma\n").is_none());
    }
}
