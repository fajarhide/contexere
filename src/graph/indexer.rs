use anyhow::Result;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MAX_FILES: usize = 5_000;
const MAX_FILE_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Default)]
pub struct GraphIndex {
    pub imports: HashMap<String, Vec<String>>,
    pub imported_by: HashMap<String, Vec<String>>,
}

impl GraphIndex {
    pub fn context_for(&self, file_path: &str) -> GraphContext {
        let normalized = normalize_path_string(file_path);
        GraphContext {
            file_path: normalized.clone(),
            imports: self.imports.get(&normalized).cloned().unwrap_or_default(),
            imported_by: self
                .imported_by
                .get(&normalized)
                .cloned()
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GraphContext {
    pub file_path: String,
    pub imports: Vec<String>,
    pub imported_by: Vec<String>,
}

pub fn build_graph(root: &Path) -> Result<GraphIndex> {
    let mut index = GraphIndex::default();
    let mut files_seen = 0usize;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !should_skip_path(e.path()))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if files_seen >= MAX_FILES {
            break;
        }
        let path = entry.path();
        if !is_supported_file(path) {
            continue;
        }
        if fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            continue;
        }
        files_seen += 1;

        let content = fs::read_to_string(path).unwrap_or_default();
        let rel = normalize_relative(root, path);
        let deps = extract_imports(path, &content);
        let resolved = resolve_imports(root, path, deps);
        if !resolved.is_empty() {
            index.imports.insert(rel.clone(), resolved.clone());
            for dep in resolved {
                index.imported_by.entry(dep).or_default().push(rel.clone());
            }
        }
    }

    dedupe_map(&mut index.imports);
    dedupe_map(&mut index.imported_by);
    Ok(index)
}

fn dedupe_map(map: &mut HashMap<String, Vec<String>>) {
    for values in map.values_mut() {
        let mut seen = HashSet::new();
        values.retain(|v| seen.insert(v.clone()));
        values.sort();
    }
}

fn should_skip_path(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        matches!(
            s.as_ref(),
            ".git" | "target" | "node_modules" | ".venv" | "dist" | "build" | ".next" | "vendor"
        )
    })
}

fn is_supported_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go")
    )
}

pub fn extract_imports(path: &Path, content: &str) -> Vec<String> {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => extract_rust_imports(content),
        "ts" | "tsx" | "js" | "jsx" => extract_jsts_imports(content),
        "py" => extract_python_imports(content),
        "go" => extract_go_imports(content),
        _ => Vec::new(),
    }
}

fn extract_rust_imports(content: &str) -> Vec<String> {
    let use_re = Regex::new(r"(?m)^\s*use\s+([a-zA-Z0-9_:]+)").unwrap();
    let mod_re = Regex::new(r"(?m)^\s*(?:pub\s+)?mod\s+([a-zA-Z0-9_]+)\s*;").unwrap();
    let mut out = Vec::new();
    for cap in use_re.captures_iter(content) {
        out.push(cap[1].to_string());
    }
    for cap in mod_re.captures_iter(content) {
        out.push(cap[1].to_string());
    }
    out
}

fn extract_jsts_imports(content: &str) -> Vec<String> {
    let import_re = Regex::new(r#"(?m)^\s*import(?:.|\n)*?from\s+['\"]([^'\"]+)['\"]"#).unwrap();
    let bare_re = Regex::new(r#"(?m)^\s*import\s+['\"]([^'\"]+)['\"]"#).unwrap();
    let require_re = Regex::new(r#"require\(\s*['\"]([^'\"]+)['\"]\s*\)"#).unwrap();
    let mut out = Vec::new();
    for re in [&import_re, &bare_re, &require_re] {
        for cap in re.captures_iter(content) {
            out.push(cap[1].to_string());
        }
    }
    out
}

fn extract_python_imports(content: &str) -> Vec<String> {
    let import_re = Regex::new(r"(?m)^\s*import\s+([a-zA-Z0-9_\.]+)").unwrap();
    let from_re = Regex::new(r"(?m)^\s*from\s+([a-zA-Z0-9_\.]+)\s+import").unwrap();
    let mut out = Vec::new();
    for cap in import_re.captures_iter(content) {
        out.push(cap[1].to_string());
    }
    for cap in from_re.captures_iter(content) {
        out.push(cap[1].to_string());
    }
    out
}

fn extract_go_imports(content: &str) -> Vec<String> {
    let re = Regex::new(r#"(?m)^\s*"([^"]+)"\s*$|import\s+"([^"]+)""#).unwrap();
    let mut out = Vec::new();
    for cap in re.captures_iter(content) {
        if let Some(m) = cap.get(1).or_else(|| cap.get(2)) {
            out.push(m.as_str().to_string());
        }
    }
    out
}

fn resolve_imports(root: &Path, from_file: &Path, imports: Vec<String>) -> Vec<String> {
    imports
        .into_iter()
        .filter_map(|imp| resolve_one(root, from_file, &imp))
        .collect()
}

fn resolve_one(root: &Path, from_file: &Path, imp: &str) -> Option<String> {
    if imp.starts_with('.') {
        let base = from_file.parent().unwrap_or(root);
        let joined = base.join(imp);
        return resolve_candidate(root, &joined);
    }

    if imp.starts_with("crate::") || imp.starts_with("super::") || imp.starts_with("self::") {
        let tail = imp
            .replace("crate::", "")
            .replace("super::", "")
            .replace("self::", "")
            .replace("::", "/");
        return resolve_candidate(root, &root.join("src").join(tail));
    }

    if imp.contains('.') && !imp.contains('/') {
        return resolve_candidate(root, &root.join(imp.replace('.', "/")));
    }

    resolve_candidate(root, &root.join(imp))
}

fn resolve_candidate(root: &Path, candidate: &Path) -> Option<String> {
    let candidates = [
        candidate.to_path_buf(),
        candidate.with_extension("rs"),
        candidate.with_extension("ts"),
        candidate.with_extension("tsx"),
        candidate.with_extension("js"),
        candidate.with_extension("jsx"),
        candidate.with_extension("py"),
        candidate.with_extension("go"),
        candidate.join("mod.rs"),
        candidate.join("index.ts"),
        candidate.join("index.tsx"),
        candidate.join("index.js"),
    ];
    // `is_file`, not `exists`: a brace-grouped `use` resolves to the bare
    // directory, which would win here instead of its `mod.rs` below.
    candidates
        .iter()
        .find(|p| p.is_file())
        .map(|p| normalize_relative(root, p))
}

fn normalize_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// One spelling per file, so the graph and anything keyed off it agree.
/// `..` is kept: dropping it would name a different file.
fn normalize_path_string(path: &str) -> String {
    use std::path::Component;

    let slashed = path.replace('\\', "/");
    let mut out = PathBuf::new();
    for component in PathBuf::from(&slashed).components() {
        match component {
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    let normalized = out.to_string_lossy().replace('\\', "/");
    if normalized.is_empty() {
        slashed
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn one_file_has_one_spelling() {
        for (given, want) in [
            ("./src/x.rs", "src/x.rs"),
            ("src/x.rs", "src/x.rs"),
            ("./src/./x.rs", "src/x.rs"),
            ("src\\x.rs", "src/x.rs"),
        ] {
            assert_eq!(normalize_path_string(given), want, "{given}");
        }
        // `..` names a different file, so it survives.
        assert_eq!(normalize_path_string("../src/x.rs"), "../src/x.rs");
        assert_eq!(normalize_path_string("."), ".");
    }

    #[test]
    fn extracts_rust_imports() {
        let imports = extract_imports(
            Path::new("lib.rs"),
            "use crate::foo::bar;\npub mod baz;\nfn main() {}",
        );
        assert!(imports.contains(&"crate::foo::bar".to_string()));
        assert!(imports.contains(&"baz".to_string()));
    }

    #[test]
    fn extracts_ts_imports() {
        let imports = extract_imports(
            Path::new("a.ts"),
            "import { x } from './b';\nconst y = require('./c');",
        );
        assert!(imports.contains(&"./b".to_string()));
        assert!(imports.contains(&"./c".to_string()));
    }

    #[test]
    fn builds_graph_for_relative_ts_imports() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.ts"), "import { b } from './b';").unwrap();
        fs::write(dir.path().join("b.ts"), "export const b = 1;").unwrap();

        let graph = build_graph(dir.path()).unwrap();
        let ctx = graph.context_for("b.ts");
        assert!(ctx.imported_by.contains(&"a.ts".to_string()));
    }

    /// A directory with no module file resolves to nothing, so
    /// `use crate::{a, b}` never becomes an edge to `src`.
    #[test]
    fn resolves_a_brace_grouped_use_to_the_module_file_not_its_directory() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/pipeline")).unwrap();
        fs::write(
            dir.path().join("src/pipeline/mod.rs"),
            "pub struct CollapseMode;\npub struct SegmentationMode;\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/registry.rs"),
            "use crate::pipeline::{CollapseMode, SegmentationMode};\n",
        )
        .unwrap();
        fs::write(dir.path().join("src/lib.rs"), "use crate::{a, b};\n").unwrap();

        let graph = build_graph(dir.path()).unwrap();

        assert!(
            graph
                .context_for("src/pipeline/mod.rs")
                .imported_by
                .contains(&"src/registry.rs".to_string()),
            "the module file owns the edge, got {:?}",
            graph.imported_by
        );
        assert!(
            !graph.imported_by.contains_key("src/pipeline"),
            "a directory is not a module and must not be a node, got {:?}",
            graph.imported_by.keys().collect::<Vec<_>>()
        );
        assert!(
            !graph.imported_by.contains_key("src"),
            "`use crate::{{a, b}}` names no single file, got {:?}",
            graph.imported_by.keys().collect::<Vec<_>>()
        );
    }
}
