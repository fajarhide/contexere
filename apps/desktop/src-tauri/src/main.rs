// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::Command;

/// The only calls the window may make: three read-only reports, and `retrieve`
/// with a handle. The page never names a program or an argument of its own.
fn allowed(args: &[String]) -> bool {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["doctor", "--json"] | ["stats", "--json"] | ["query", "timeline", "today", "--json"] => {
            true
        }
        ["retrieve", handle] => handle.len() == 16 && handle.bytes().all(|b| b.is_ascii_hexdigit()),
        _ => false,
    }
}

/// A desktop app on macOS does not inherit the shell's PATH, so the places the
/// installers use are tried after it.
fn find_contexere() -> Option<PathBuf> {
    let name = if cfg!(windows) { "contexere.exe" } else { "contexere" };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".cargo").join("bin"));
    }
    dirs.extend(
        ["/opt/homebrew/bin", "/usr/local/bin", "/home/linuxbrew/.linuxbrew/bin"].map(PathBuf::from),
    );
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

#[tauri::command]
async fn contexere(args: Vec<String>) -> Result<String, String> {
    if !allowed(&args) {
        return Err(format!("not a call this app makes: {args:?}"));
    }
    let bin = find_contexere()
        .ok_or_else(|| "missing:no contexere on PATH or in the usual install places".to_string())?;
    let out = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = Command::new(bin);
        cmd.args(&args).env("CONTEXERE_QUIET", "1");
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
        cmd.output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    // `doctor` exits non-zero when something is unhealthy and still prints its report.
    if out.stdout.is_empty() && !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![contexere])
        .run(tauri::generate_context!())
        .expect("the Contexere window could not start");
}

#[cfg(test)]
mod tests {
    use super::allowed;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn allows_the_reports_and_a_retrieve_by_handle() {
        assert!(allowed(&args(&["doctor", "--json"])));
        assert!(allowed(&args(&["query", "timeline", "today", "--json"])));
        assert!(allowed(&args(&["retrieve", "4bcb6a0f7dda2d84"])));
    }

    #[test]
    fn refuses_anything_that_writes_or_takes_free_text() {
        assert!(!allowed(&args(&["doctor", "--json", "--fix"])));
        assert!(!allowed(&args(&["reset", "--all"])));
        assert!(!allowed(&args(&["retrieve", "--all"])));
        assert!(!allowed(&args(&["retrieve", "4bcb6a0f7dda2d8"])));
        assert!(!allowed(&args(&["retrieve", "../../etc/passwd"])));
        assert!(!allowed(&args(&[])));
    }
}
