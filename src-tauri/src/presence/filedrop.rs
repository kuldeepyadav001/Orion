//! File-drop intake: deciding what Orion will accept when a user drops files
//! on the window, the tray, or passes them on the command line.
//!
//! This is a **trust boundary**. Dropped paths come from the file manager, a
//! browser download, a chat client, or an attacker who convinced the user to
//! drag something. Before any of it reaches the ingestion pipeline we decide
//! what is worth opening and what is refused, with a reason the user can act
//! on.
//!
//! The rules are conservative on purpose. Silently ignoring a dropped file is
//! the worst outcome: the user thinks Orion has their document when it does
//! not, and only discovers otherwise when an answer is wrong.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Largest file we will accept from a drop. Matches the ingestion cap.
pub const MAX_DROP_BYTES: u64 = 64 * 1024 * 1024;

/// Most files accepted from one drop. A user dragging a folder of 10,000
/// files should get a clear refusal, not a frozen window.
pub const MAX_DROP_COUNT: usize = 200;

/// How deep we walk into a dropped directory.
pub const MAX_DIR_DEPTH: usize = 8;

/// File types Orion will accept from a drop.
///
/// This is deliberately a **local allowlist**, not a re-export of the
/// ingestion pipeline's `Format`. Drop filtering is a policy question — what
/// the user is allowed to hand us — while `Format` is a parser question. They
/// agree today and may not always: a format can be parseable but unwanted
/// from a drag-and-drop (an enormous log, say), or accepted at drop time and
/// routed to a converter later.
///
/// Keeping them separate also keeps this module free of the PDF and ZIP
/// dependencies, which matters because M3 must be buildable and testable on
/// its own branch.
pub const ACCEPTED_EXTENSIONS: &[&str] = &[
    // documents
    "pdf", "docx", "xlsx", "xlsm", "csv", "tsv", "md", "markdown", "html", "htm", "txt", "log",
    "rst", // code and config
    "rs", "py", "js", "ts", "jsx", "tsx", "go", "java", "c", "h", "cpp", "hpp", "cs", "rb", "php",
    "swift", "kt", "sh", "sql", "toml", "yaml", "yml", "json",
];

/// Is this extension one we accept?
pub fn is_accepted(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .map(|e| ACCEPTED_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or(false)
}

/// Outcome for one dropped path.
#[derive(Debug, Clone, PartialEq)]
pub enum DropVerdict {
    /// Will be ingested.
    Accept { path: PathBuf },
    /// Refused, with a reason fit to show the user verbatim.
    Reject { path: PathBuf, reason: String },
}

impl DropVerdict {
    pub fn is_accept(&self) -> bool {
        matches!(self, DropVerdict::Accept { .. })
    }

    pub fn path(&self) -> &Path {
        match self {
            DropVerdict::Accept { path } => path,
            DropVerdict::Reject { path, .. } => path,
        }
    }
}

/// The result of assessing an entire drop.
#[derive(Debug, Clone, Default)]
pub struct DropAssessment {
    pub accepted: Vec<PathBuf>,
    pub rejected: Vec<(PathBuf, String)>,
    /// True when the drop was cut short by `MAX_DROP_COUNT`.
    pub truncated: bool,
}

impl DropAssessment {
    pub fn is_empty(&self) -> bool {
        self.accepted.is_empty() && self.rejected.is_empty()
    }

    /// One-line summary for the UI.
    pub fn summary(&self) -> String {
        let a = self.accepted.len();
        let r = self.rejected.len();

        let mut s = match a {
            0 => "No files could be added".to_string(),
            1 => "Adding 1 file".to_string(),
            n => format!("Adding {n} files"),
        };
        if r > 0 {
            s.push_str(&format!(", skipped {r}"));
        }
        if self.truncated {
            s.push_str(&format!(" (stopped at the {MAX_DROP_COUNT}-file limit)"));
        }
        s
    }
}

/// Metadata the assessor needs, abstracted so the logic is testable without
/// touching a real filesystem.
pub trait FileProbe {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn len(&self, path: &Path) -> Option<u64>;
    /// Entries of a directory, in any order.
    fn read_dir(&self, path: &Path) -> Vec<PathBuf>;
    /// True when the path is a symlink. We resolve nothing and refuse them.
    fn is_symlink(&self, path: &Path) -> bool;
}

/// Real filesystem implementation.
pub struct RealFs;

impl FileProbe for RealFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
    fn len(&self, path: &Path) -> Option<u64> {
        std::fs::metadata(path).ok().map(|m| m.len())
    }
    fn read_dir(&self, path: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(path)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default()
    }
    fn is_symlink(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }
}

/// Assess a set of dropped paths.
pub fn assess<P: FileProbe>(paths: &[PathBuf], probe: &P) -> DropAssessment {
    let mut out = DropAssessment::default();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut queue: Vec<(PathBuf, usize)> = paths.iter().map(|p| (p.clone(), 0)).collect();

    while let Some((path, depth)) = queue.pop() {
        if out.accepted.len() + out.rejected.len() >= MAX_DROP_COUNT {
            out.truncated = true;
            break;
        }

        // Deduplicate: dropping a folder and a file inside it is common.
        if !seen.insert(path.clone()) {
            continue;
        }

        match verdict(&path, depth, probe) {
            Verdict::Accept => out.accepted.push(path),
            Verdict::Reject(reason) => out.rejected.push((path, reason)),
            Verdict::Descend => {
                for child in probe.read_dir(&path) {
                    queue.push((child, depth + 1));
                }
            }
            Verdict::Skip => {}
        }
    }

    // Stable order so the UI does not shuffle between runs.
    out.accepted.sort();
    out.rejected.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

enum Verdict {
    Accept,
    Reject(String),
    Descend,
    /// Ignored silently — noise the user did not mean to drop.
    Skip,
}

fn verdict<P: FileProbe>(path: &Path, depth: usize, probe: &P) -> Verdict {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Symlinks are refused rather than followed. Following them lets a
    // dropped folder reach anywhere on disk, including places the user did
    // not intend to share with an AI that will quote their contents back.
    if probe.is_symlink(path) {
        return Verdict::Reject(format!(
            "{name} is a shortcut or symbolic link; drop the real file instead"
        ));
    }

    if !probe.exists(path) {
        return Verdict::Reject(format!("{name} no longer exists"));
    }

    if probe.is_dir(path) {
        if depth >= MAX_DIR_DEPTH {
            return Verdict::Reject(format!(
                "{name} is nested deeper than {MAX_DIR_DEPTH} folders; \
                 drop the files you want directly"
            ));
        }
        if is_noise_dir(&name) {
            return Verdict::Skip;
        }
        return Verdict::Descend;
    }

    // Hidden and system files are skipped quietly. A user dropping a folder
    // does not mean to ingest .DS_Store or .gitignore, and complaining about
    // each one buries the real messages.
    if name.starts_with('.') {
        return Verdict::Skip;
    }

    if !is_accepted(path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_else(|| "no extension".to_string());
        return Verdict::Reject(format!("{name} ({ext}) is not a file type Orion can read"));
    }

    match probe.len(path) {
        Some(0) => Verdict::Reject(format!("{name} is empty")),
        Some(n) if n > MAX_DROP_BYTES => Verdict::Reject(format!(
            "{name} is {} MB, above the {} MB limit",
            n / 1024 / 1024,
            MAX_DROP_BYTES / 1024 / 1024
        )),
        Some(_) => Verdict::Accept,
        None => Verdict::Reject(format!("{name} could not be read")),
    }
}

/// Directories that are never worth walking into.
fn is_noise_dir(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | "target"
            | ".git"
            | ".svn"
            | "__pycache__"
            | ".venv"
            | "venv"
            | "dist"
            | "build"
            | ".next"
            | ".cache"
            | "vendor"
            | ".idea"
            | ".vscode"
    ) || name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory filesystem for deterministic tests.
    #[derive(Default)]
    struct FakeFs {
        files: Vec<(PathBuf, u64)>,
        dirs: Vec<PathBuf>,
        symlinks: Vec<PathBuf>,
    }

    impl FakeFs {
        fn file(mut self, p: &str, len: u64) -> Self {
            self.files.push((PathBuf::from(p), len));
            self
        }
        fn dir(mut self, p: &str) -> Self {
            self.dirs.push(PathBuf::from(p));
            self
        }
        fn symlink(mut self, p: &str) -> Self {
            self.symlinks.push(PathBuf::from(p));
            self
        }
    }

    impl FileProbe for FakeFs {
        fn exists(&self, path: &Path) -> bool {
            self.files.iter().any(|(p, _)| p == path)
                || self.dirs.iter().any(|p| p == path)
                || self.symlinks.iter().any(|p| p == path)
        }
        fn is_dir(&self, path: &Path) -> bool {
            self.dirs.iter().any(|p| p == path)
        }
        fn len(&self, path: &Path) -> Option<u64> {
            self.files.iter().find(|(p, _)| p == path).map(|(_, l)| *l)
        }
        fn read_dir(&self, path: &Path) -> Vec<PathBuf> {
            let mut out: Vec<PathBuf> = Vec::new();
            for (p, _) in &self.files {
                if p.parent() == Some(path) {
                    out.push(p.clone());
                }
            }
            for p in &self.dirs {
                if p.parent() == Some(path) {
                    out.push(p.clone());
                }
            }
            out
        }
        fn is_symlink(&self, path: &Path) -> bool {
            self.symlinks.iter().any(|p| p == path)
        }
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    /* ---------- the happy path ---------- */

    #[test]
    fn a_supported_file_is_accepted() {
        let fs = FakeFs::default().file("/docs/report.pdf", 1024);
        let a = assess(&[p("/docs/report.pdf")], &fs);
        assert_eq!(a.accepted.len(), 1);
        assert_eq!(a.accepted[0], p("/docs/report.pdf"));
        assert!(a.rejected.is_empty());
    }

    #[test]
    fn every_supported_format_is_accepted() {
        let names = [
            "a.pdf", "b.docx", "c.xlsx", "d.csv", "e.md", "f.html", "g.txt", "h.rs",
        ];
        let mut fs = FakeFs::default();
        for n in names {
            fs = fs.file(&format!("/d/{n}"), 100);
        }
        let paths: Vec<PathBuf> = names.iter().map(|n| p(&format!("/d/{n}"))).collect();
        let a = assess(&paths, &fs);
        assert_eq!(a.accepted.len(), names.len(), "rejected: {:?}", a.rejected);
    }

    /* ---------- rejections the user must see ---------- */

    #[test]
    fn an_unsupported_type_is_rejected_with_its_extension() {
        let fs = FakeFs::default().file("/d/photo.heic", 1024);
        let a = assess(&[p("/d/photo.heic")], &fs);
        assert_eq!(a.accepted.len(), 0);
        assert_eq!(a.rejected.len(), 1);
        assert!(
            a.rejected[0].1.contains(".heic"),
            "reason should name the type: {}",
            a.rejected[0].1
        );
    }

    #[test]
    fn an_empty_file_is_rejected_rather_than_silently_indexed() {
        let fs = FakeFs::default().file("/d/empty.md", 0);
        let a = assess(&[p("/d/empty.md")], &fs);
        assert_eq!(a.rejected.len(), 1);
        assert!(a.rejected[0].1.contains("empty"));
    }

    #[test]
    fn an_oversized_file_is_rejected_with_both_numbers() {
        let fs = FakeFs::default().file("/d/huge.pdf", MAX_DROP_BYTES + 1);
        let a = assess(&[p("/d/huge.pdf")], &fs);
        assert_eq!(a.rejected.len(), 1);
        let r = &a.rejected[0].1;
        assert!(r.contains("64"), "should state the limit: {r}");
    }

    #[test]
    fn a_missing_file_is_rejected_not_ignored() {
        let fs = FakeFs::default();
        let a = assess(&[p("/gone/file.pdf")], &fs);
        assert_eq!(a.rejected.len(), 1);
        assert!(a.rejected[0].1.contains("no longer exists"));
    }

    /* ---------- security ---------- */

    #[test]
    fn symlinks_are_refused_not_followed() {
        // Following a symlink lets a dropped folder reach anywhere on disk.
        let fs = FakeFs::default()
            .symlink("/d/innocent.pdf")
            .file("/etc/shadow", 100);
        let a = assess(&[p("/d/innocent.pdf")], &fs);
        assert_eq!(a.accepted.len(), 0, "symlink was followed");
        assert_eq!(a.rejected.len(), 1);
        assert!(a.rejected[0].1.contains("link"));
    }

    #[test]
    fn a_symlinked_directory_is_not_walked() {
        let fs = FakeFs::default()
            .symlink("/d/link")
            .dir("/d/link")
            .file("/d/link/secret.md", 100);
        let a = assess(&[p("/d/link")], &fs);
        assert!(a.accepted.is_empty(), "walked into a symlinked directory");
    }

    #[test]
    fn the_file_count_is_bounded() {
        let mut fs = FakeFs::default().dir("/big");
        for i in 0..(MAX_DROP_COUNT + 50) {
            fs = fs.file(&format!("/big/f{i}.md"), 10);
        }
        let a = assess(&[p("/big")], &fs);
        assert!(a.accepted.len() + a.rejected.len() <= MAX_DROP_COUNT);
        assert!(a.truncated, "truncation must be reported, not silent");
        assert!(a.summary().contains("limit"));
    }

    #[test]
    fn directory_recursion_is_depth_bounded() {
        // A pathological tree must not recurse without limit.
        let mut fs = FakeFs::default();
        let mut path = String::from("/deep");
        fs = fs.dir(&path);
        for i in 0..(MAX_DIR_DEPTH + 5) {
            path.push_str(&format!("/d{i}"));
            fs = fs.dir(&path);
        }
        fs = fs.file(&format!("{path}/buried.md"), 10);

        let a = assess(&[p("/deep")], &fs);
        assert!(
            a.rejected.iter().any(|(_, r)| r.contains("nested")),
            "depth limit was not reported: {a:?}"
        );
    }

    /* ---------- directory walking ---------- */

    #[test]
    fn a_directory_is_walked_for_supported_files() {
        let fs = FakeFs::default()
            .dir("/proj")
            .file("/proj/readme.md", 100)
            .file("/proj/notes.txt", 100)
            .file("/proj/image.png", 100);
        let a = assess(&[p("/proj")], &fs);
        assert_eq!(a.accepted.len(), 2);
        assert_eq!(a.rejected.len(), 1, "png should be reported, not hidden");
    }

    #[test]
    fn noise_directories_are_skipped_silently() {
        let fs = FakeFs::default()
            .dir("/proj")
            .dir("/proj/node_modules")
            .file("/proj/node_modules/pkg.md", 100)
            .dir("/proj/.git")
            .file("/proj/.git/config.txt", 100)
            .file("/proj/real.md", 100);

        let a = assess(&[p("/proj")], &fs);
        assert_eq!(a.accepted.len(), 1, "walked into noise: {:?}", a.accepted);
        assert_eq!(a.accepted[0], p("/proj/real.md"));
        assert!(
            a.rejected.is_empty(),
            "noise must be silent, not reported: {:?}",
            a.rejected
        );
    }

    #[test]
    fn hidden_files_are_skipped_silently() {
        let fs = FakeFs::default()
            .dir("/d")
            .file("/d/.DS_Store", 100)
            .file("/d/.gitignore", 100)
            .file("/d/real.md", 100);
        let a = assess(&[p("/d")], &fs);
        assert_eq!(a.accepted.len(), 1);
        assert!(
            a.rejected.is_empty(),
            "hidden files should not nag the user"
        );
    }

    /* ---------- deduplication and ordering ---------- */

    #[test]
    fn dropping_a_folder_and_a_file_inside_it_does_not_duplicate() {
        let fs = FakeFs::default().dir("/d").file("/d/a.md", 100);
        let a = assess(&[p("/d"), p("/d/a.md")], &fs);
        assert_eq!(a.accepted.len(), 1, "duplicate: {:?}", a.accepted);
    }

    #[test]
    fn the_same_path_twice_is_deduplicated() {
        let fs = FakeFs::default().file("/d/a.md", 100);
        let a = assess(&[p("/d/a.md"), p("/d/a.md"), p("/d/a.md")], &fs);
        assert_eq!(a.accepted.len(), 1);
    }

    #[test]
    fn results_are_in_a_stable_order() {
        let fs = FakeFs::default()
            .file("/d/c.md", 10)
            .file("/d/a.md", 10)
            .file("/d/b.md", 10);
        let paths = vec![p("/d/c.md"), p("/d/a.md"), p("/d/b.md")];
        let first = assess(&paths, &fs);
        let second = assess(&paths, &fs);
        assert_eq!(first.accepted, second.accepted);
        assert_eq!(first.accepted[0], p("/d/a.md"), "not sorted");
    }

    /* ---------- summary text ---------- */

    #[test]
    fn the_summary_reads_naturally() {
        let fs = FakeFs::default()
            .file("/d/a.md", 10)
            .file("/d/b.pdf", 10)
            .file("/d/c.png", 10);
        let a = assess(&[p("/d/a.md"), p("/d/b.pdf"), p("/d/c.png")], &fs);
        assert_eq!(a.summary(), "Adding 2 files, skipped 1");
    }

    #[test]
    fn the_summary_uses_the_singular_for_one_file() {
        let fs = FakeFs::default().file("/d/a.md", 10);
        assert_eq!(assess(&[p("/d/a.md")], &fs).summary(), "Adding 1 file");
    }

    #[test]
    fn the_summary_is_honest_when_nothing_worked() {
        let fs = FakeFs::default().file("/d/a.png", 10);
        let s = assess(&[p("/d/a.png")], &fs).summary();
        assert!(s.starts_with("No files"), "{s}");
    }

    #[test]
    fn an_empty_drop_is_empty_not_a_crash() {
        let a = assess(&[], &FakeFs::default());
        assert!(a.is_empty());
    }

    #[test]
    fn odd_paths_do_not_panic() {
        let fs = FakeFs::default();
        for s in [
            "",
            "/",
            "..",
            "//////",
            "\u{0}",
            "🙂/🙂.md",
            &"a/".repeat(300),
        ] {
            let _ = assess(&[p(s)], &fs);
        }
    }
}
