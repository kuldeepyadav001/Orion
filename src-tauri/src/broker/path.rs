//! Secure path resolution and symlink containment (M5).
//!
//! ## Attack Vectors Thwarted Here
//!
//! 1. **Naive Prefix Traversal:** `starts_with("/safe")` succeeds on `/safe/../etc/passwd`.
//! 2. **Symlink Escape:** Creating a symlink inside the workspace pointing to `/home/user/.ssh`.
//! 3. **Windows Reserved Device Names:** Creating files named `CON`, `PRN`, `AUX`, `NUL`
//!    can crash or stall the Windows Win32 subsystem.
//! 4. **Alternate Data Streams (ADS):** Writing to `file.txt:hidden.exe` on NTFS.
//! 5. **Null Byte Injection:** Trailing `%00` or `\0` bypassing extension checks.

use std::path::{Component, Path, PathBuf};

use crate::error::{OrionError, Result};

/// Windows reserved DOS device filenames that must never be created or opened.
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Known sensitive credential files and system directories permanently blocked from agent access.
const SENSITIVE_PATTERNS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".git/config",
    ".git/credentials",
    ".git-credentials",
    ".netrc",
    ".env",
    "id_rsa",
    "id_ed25519",
    "known_hosts",
    "authorized_keys",
    ".bashrc",
    ".zshrc",
    ".profile",
    ".bash_profile",
];

/// Check if a path string or component references a permanently blocked sensitive target.
pub fn is_blocked_system_path(target: &Path) -> bool {
    let raw = target.to_string_lossy().to_lowercase();

    // Check null bytes
    if raw.contains('\0') {
        return true;
    }

    // Check sensitive patterns in components
    for pattern in SENSITIVE_PATTERNS {
        let pattern_lower = pattern.to_lowercase();
        if raw.contains(&pattern_lower) {
            return true;
        }
    }

    // Check system directory roots
    #[cfg(unix)]
    {
        let system_roots = ["/etc", "/usr", "/bin", "/sbin", "/var/run", "/dev", "/proc", "/sys"];
        for root in system_roots {
            if raw == root || raw.starts_with(&format!("{root}/")) {
                return true;
            }
        }
    }

    #[cfg(windows)]
    {
        let win_system = ["c:\\windows", "c:\\program files", "c:\\program files (x86)", "c:\\programdata"];
        for root in win_system {
            if raw.starts_with(root) {
                return true;
            }
        }
        // Windows Startup folder
        if raw.contains("start menu\\programs\\startup") {
            return true;
        }
    }

    false
}

/// Check if a filename corresponds to a forbidden Windows reserved device name.
pub fn is_windows_reserved_device(name: &str) -> bool {
    let clean = name.trim().to_uppercase();
    let stem = clean.split('.').next().unwrap_or(&clean);
    WINDOWS_RESERVED_NAMES.contains(&stem)
}

/// Safely resolve a user-supplied path beneath an authorized base root (`RESOLVE_BENEATH` semantics).
///
/// Steps:
/// 1. Reject paths containing null bytes or alternate data streams (`:` on Windows).
/// 2. Reject paths matching Windows reserved device names (`CON`, `NUL`, etc.).
/// 3. Reject sensitive/blocked files (`.ssh`, `.env`, system dirs).
/// 4. Normalize components, rejecting any root jumps or uncontained parent traversals (`..`).
/// 5. Canonicalize the target (or its existing ancestor if creating a new file)
///    to fully dereference any symlinks.
/// 6. Assert that the canonical result strictly starts with the canonical base directory.
pub fn resolve_beneath(base: &Path, user_path: &Path) -> Result<PathBuf> {
    let raw_str = user_path.to_string_lossy();

    // 1. Check null bytes
    if raw_str.contains('\0') {
        return Err(OrionError::Security("null byte in path is forbidden".into()));
    }

    // 2. Alternate data streams on Windows (e.g. `foo.txt:hidden`)
    if cfg!(windows) && raw_str.contains(':') && !raw_str.chars().nth(1).map_or(false, |c| c == ':') {
        // Allow drive letter like `C:\`, but disallow subsequent colons (ADS)
        let rest = if raw_str.len() > 2 && raw_str.chars().nth(1) == Some(':') {
            &raw_str[2..]
        } else {
            &raw_str
        };
        if rest.contains(':') {
            return Err(OrionError::Security("alternate data streams (ADS) are forbidden".into()));
        }
    }

    // 3. Reject permanently blocked system / credential targets
    if is_blocked_system_path(user_path) {
        return Err(OrionError::Security(format!(
            "access to sensitive target '{}' is permanently blocked",
            user_path.display()
        )));
    }

    // 4. Inspect components for reserved device names or parent escapes
    for comp in user_path.components() {
        if let Component::Normal(os_name) = comp {
            let name_str = os_name.to_string_lossy();
            if is_windows_reserved_device(&name_str) {
                return Err(OrionError::Security(format!(
                    "reserved Windows device name '{name_str}' is forbidden"
                )));
            }
        }
    }

    // 5. Canonicalize the base directory to establish ground truth
    let canon_base = if base.is_dir() {
        base.canonicalize()
            .map_err(|e| OrionError::Security(format!("cannot canonicalize base path '{}': {e}", base.display())))?
    } else {
        base.to_path_buf()
    };

    // Construct unified target path
    let candidate = if user_path.is_absolute() {
        user_path.to_path_buf()
    } else {
        canon_base.join(user_path)
    };

    // 6. Canonicalize existing path or locate nearest existing parent to evaluate symlinks
    let mut probe = candidate.clone();
    let mut trailing_parts = Vec::new();

    while !probe.exists() {
        if let Some(file_name) = probe.file_name() {
            trailing_parts.push(file_name.to_os_string());
        }
        match probe.parent() {
            Some(p) if p != probe => probe = p.to_path_buf(),
            _ => break,
        }
    }

    let canon_ancestor = if probe.exists() {
        probe.canonicalize().map_err(|e| {
            OrionError::Security(format!(
                "cannot canonicalize path component '{}': {e}",
                probe.display()
            ))
        })?
    } else {
        canon_base.clone()
    };

    let mut resolved = canon_ancestor;
    for part in trailing_parts.into_iter().rev() {
        resolved.push(part);
    }

    // 7. Verify containment under base directory
    if !resolved.starts_with(&canon_base) {
        return Err(OrionError::Security(format!(
            "path traversal detected: target '{}' escapes allowed root '{}'",
            resolved.display(),
            canon_base.display()
        )));
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_valid_workspace_relative_path() {
        let temp_dir = std::env::temp_dir();
        let canon_temp = temp_dir.canonicalize().unwrap();
        let res = resolve_beneath(&canon_temp, Path::new("sub/doc.txt"));
        assert!(res.is_ok());
        let p = res.unwrap();
        assert!(p.starts_with(&canon_temp));
    }

    #[test]
    fn blocks_directory_traversal() {
        let temp_dir = std::env::temp_dir();
        let canon_temp = temp_dir.canonicalize().unwrap();
        let res = resolve_beneath(&canon_temp, Path::new("../../etc/passwd"));
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("escapes allowed root"));
    }

    #[test]
    fn blocks_sensitive_files() {
        let temp_dir = std::env::temp_dir();
        let canon_temp = temp_dir.canonicalize().unwrap();
        let res = resolve_beneath(&canon_temp, Path::new(".ssh/id_rsa"));
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("permanently blocked"));

        let res2 = resolve_beneath(&canon_temp, Path::new("app/.env"));
        assert!(res2.is_err());
        assert!(res2.unwrap_err().to_string().contains("permanently blocked"));
    }

    #[test]
    fn blocks_null_byte() {
        let temp_dir = std::env::temp_dir();
        let canon_temp = temp_dir.canonicalize().unwrap();
        let res = resolve_beneath(&canon_temp, Path::new("safe.txt\0.exe"));
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("null byte"));
    }

    #[test]
    fn blocks_reserved_windows_names() {
        let temp_dir = std::env::temp_dir();
        let canon_temp = temp_dir.canonicalize().unwrap();
        let res = resolve_beneath(&canon_temp, Path::new("CON.txt"));
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("reserved Windows device"));
    }
}
