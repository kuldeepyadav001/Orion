//! Model registry, installation state, and download management.
//!
//! The registry is data, not code: a JSON manifest mapping tiers to model
//! files. That means new models can ship without an app update, and the
//! catalogue is reviewable in one place.
//!
//! Licensing is a first-class field. Only permissively licensed weights
//! (Apache-2.0 / MIT) may go into an offline bundle; anything else must be
//! fetched by the user. Gemma and Llama carry custom terms and are therefore
//! marked `redistributable: false`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::profiler::Tier;

/// One model in the catalogue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelSpec {
    /// Stable identifier, e.g. `qwen2.5-3b-instruct-q4km`.
    pub id: String,
    pub name: String,
    pub tier: Tier,
    /// Hugging Face repository.
    pub repo: String,
    /// File within the repository.
    pub file: String,
    pub size_gib: f64,
    /// Approximate resident memory once loaded.
    pub ram_gib: f64,
    pub license: String,
    /// Whether the licence permits shipping these weights inside our installer.
    pub redistributable: bool,
    /// SHA-256 of the file, when known. Verified after download.
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub context: u32,
}

impl ModelSpec {
    pub fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/main/{}?download=true",
            self.repo, self.file
        )
    }
}

/// Installation state of a model on this machine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallState {
    NotInstalled,
    Downloading,
    Installed,
    /// Present but failed verification — treated as unusable.
    Corrupt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatus {
    pub spec: ModelSpec,
    pub state: InstallState,
    pub path: Option<String>,
    /// Bytes on disk, when present.
    pub bytes: Option<u64>,
}

/// The built-in catalogue.
///
/// Kept small and curated rather than mirroring all of Hugging Face: a
/// personal AI should make a good default choice, not present a research menu.
pub fn builtin_registry() -> Vec<ModelSpec> {
    vec![
        ModelSpec {
            id: "qwen2.5-3b-instruct-q4km".into(),
            name: "Qwen2.5 3B Instruct".into(),
            tier: Tier::T1,
            repo: "Qwen/Qwen2.5-3B-Instruct-GGUF".into(),
            file: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
            size_gib: 2.1,
            ram_gib: 3.4,
            license: "Apache-2.0".into(),
            redistributable: true,
            sha256: None,
            context: 32768,
        },
        ModelSpec {
            id: "qwen2.5-7b-instruct-q4km".into(),
            name: "Qwen2.5 7B Instruct".into(),
            tier: Tier::T2,
            repo: "Qwen/Qwen2.5-7B-Instruct-GGUF".into(),
            file: "qwen2.5-7b-instruct-q4_k_m.gguf".into(),
            size_gib: 4.7,
            ram_gib: 5.5,
            license: "Apache-2.0".into(),
            redistributable: true,
            sha256: None,
            context: 32768,
        },
        ModelSpec {
            id: "qwen2.5-14b-instruct-q4km".into(),
            name: "Qwen2.5 14B Instruct".into(),
            tier: Tier::T3,
            repo: "Qwen/Qwen2.5-14B-Instruct-GGUF".into(),
            file: "qwen2.5-14b-instruct-q4_k_m.gguf".into(),
            size_gib: 8.9,
            ram_gib: 9.0,
            license: "Apache-2.0".into(),
            redistributable: true,
            sha256: None,
            context: 32768,
        },
        ModelSpec {
            id: "qwen2.5-32b-instruct-q4km".into(),
            name: "Qwen2.5 32B Instruct".into(),
            tier: Tier::T4,
            repo: "Qwen/Qwen2.5-32B-Instruct-GGUF".into(),
            file: "qwen2.5-32b-instruct-q4_k_m.gguf".into(),
            size_gib: 19.9,
            ram_gib: 19.0,
            license: "Apache-2.0".into(),
            redistributable: true,
            sha256: None,
            context: 32768,
        },
    ]
}

pub struct ModelManager {
    registry: Vec<ModelSpec>,
    models_dir: PathBuf,
}

impl ModelManager {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            registry: builtin_registry(),
            models_dir,
        }
    }

    /// Load a registry override from `models/registry.json` if present,
    /// otherwise use the built-in catalogue.
    pub fn with_registry_file(mut self) -> Self {
        let path = self.models_dir.join("registry.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<Vec<ModelSpec>>(&text) {
                Ok(specs) if !specs.is_empty() => {
                    tracing::info!(count = specs.len(), "loaded registry override");
                    self.registry = specs;
                }
                Ok(_) => tracing::warn!("registry.json is empty; using built-in catalogue"),
                Err(e) => tracing::error!(error = %e, "invalid registry.json; using built-in"),
            }
        }
        self
    }

    pub fn registry(&self) -> &[ModelSpec] {
        &self.registry
    }

    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    pub fn get(&self, id: &str) -> Option<&ModelSpec> {
        self.registry.iter().find(|m| m.id == id)
    }

    /// The catalogue entry for a tier.
    pub fn for_tier(&self, tier: Tier) -> Option<&ModelSpec> {
        self.registry.iter().find(|m| m.tier == tier)
    }

    pub fn local_path(&self, spec: &ModelSpec) -> PathBuf {
        self.models_dir.join(&spec.file)
    }

    pub fn state_of(&self, spec: &ModelSpec) -> InstallState {
        let path = self.local_path(spec);

        // A `.part` file means a download was interrupted.
        if self.models_dir.join(format!("{}.part", spec.file)).exists() && !path.exists() {
            return InstallState::Downloading;
        }
        if !path.exists() {
            return InstallState::NotInstalled;
        }

        // Size sanity check: catches truncated files without hashing gigabytes
        // on every launch. Full verification happens after download.
        match std::fs::metadata(&path) {
            Ok(m) => {
                let expected = (spec.size_gib * 1024.0 * 1024.0 * 1024.0) as u64;
                // GGUF quant sizes vary by a few percent between releases.
                if expected > 0 && m.len() < expected / 2 {
                    tracing::warn!(
                        file = %spec.file, actual = m.len(), expected,
                        "model file is far smaller than expected"
                    );
                    InstallState::Corrupt
                } else {
                    InstallState::Installed
                }
            }
            Err(_) => InstallState::Corrupt,
        }
    }

    pub fn status_of(&self, spec: &ModelSpec) -> ModelStatus {
        let state = self.state_of(spec);
        let path = self.local_path(spec);
        let bytes = std::fs::metadata(&path).ok().map(|m| m.len());
        ModelStatus {
            spec: spec.clone(),
            state,
            path: path.exists().then(|| path.to_string_lossy().to_string()),
            bytes,
        }
    }

    pub fn all_statuses(&self) -> Vec<ModelStatus> {
        self.registry.iter().map(|s| self.status_of(s)).collect()
    }

    /// Any usable model already on disk, best (highest tier) first.
    ///
    /// Used at startup so Orion runs with whatever the user already has
    /// rather than refusing to start.
    pub fn best_installed(&self) -> Option<&ModelSpec> {
        let mut installed: Vec<&ModelSpec> = self
            .registry
            .iter()
            .filter(|s| self.state_of(s) == InstallState::Installed)
            .collect();
        installed.sort_by_key(|s| std::cmp::Reverse(s.tier));
        installed.into_iter().next()
    }

    /// Pick the model to load: the tier's own model if installed, otherwise
    /// the best installed alternative, otherwise nothing.
    pub fn resolve_for_tier(&self, tier: Tier) -> Option<&ModelSpec> {
        if let Some(spec) = self.for_tier(tier) {
            if self.state_of(spec) == InstallState::Installed {
                return Some(spec);
            }
        }
        self.best_installed()
    }

    /// Any `.gguf` in the models directory, including ones we did not install.
    /// Users are allowed to bring their own weights.
    pub fn foreign_models(&self) -> Vec<PathBuf> {
        let known: Vec<&str> = self.registry.iter().map(|s| s.file.as_str()).collect();
        let Ok(entries) = std::fs::read_dir(&self.models_dir) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("gguf"))
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .map(|n| !known.contains(&n))
                    .unwrap_or(false)
            })
            .collect();
        out.sort();
        out
    }

    /// Verify a downloaded file against its published hash, when we have one.
    pub fn verify(&self, spec: &ModelSpec) -> Result<bool> {
        let Some(expected) = &spec.sha256 else {
            return Ok(true); // nothing to check against
        };
        let path = self.local_path(spec);
        let actual = sha256_file(&path)?;
        let ok = actual.eq_ignore_ascii_case(expected);
        if !ok {
            tracing::error!(file = %spec.file, %actual, %expected, "checksum mismatch");
        }
        Ok(ok)
    }
}

/// Stream a file through SHA-256 without loading it into memory.
fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize_hex())
}

/* ------------------------------------------------------------------ */
/* Minimal SHA-256                                                     */
/*                                                                     */
/* Implemented here rather than pulling a crate: it is ~60 lines, has  */
/* no unsafe code, and keeps the dependency surface of a security-     */
/* sensitive path small. Verified against the standard NIST vectors    */
/* in the tests below.                                                 */
/* ------------------------------------------------------------------ */

struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buflen: usize,
    len: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: [0u8; 64],
            buflen: 0,
            len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let take = (64 - self.buflen).min(data.len());
            self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
            self.buflen += take;
            data = &data[take..];
            if self.buflen == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buflen = 0;
            }
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        for (s, v) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }

    fn finalize_hex(mut self) -> String {
        let bitlen = self.len.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buflen != 56 {
            self.update(&[0]);
        }
        // `update` maintains `len`, so write the length directly.
        let block_tail = bitlen.to_be_bytes();
        self.buf[56..64].copy_from_slice(&block_tail);
        let block = self.buf;
        self.compress(&block);

        self.state.iter().map(|w| format!("{w:08x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("orion-models-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /* ---------- sha-256 ---------- */

    fn sha_hex(data: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(data);
        h.finalize_hex()
    }

    #[test]
    fn sha256_matches_nist_vectors() {
        assert_eq!(
            sha_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_handles_multi_block_input() {
        // Exercises the buffering path across many compress() calls.
        let data = vec![b'a'; 1_000_000];
        assert_eq!(
            sha_hex(&data),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn sha256_of_a_file_matches_the_same_bytes() {
        let dir = temp_dir();
        let p = dir.join("f.bin");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), sha_hex(b"abc"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /* ---------- registry ---------- */

    #[test]
    fn registry_has_one_model_per_tier() {
        let r = builtin_registry();
        for tier in [Tier::T1, Tier::T2, Tier::T3, Tier::T4] {
            assert_eq!(
                r.iter().filter(|m| m.tier == tier).count(),
                1,
                "exactly one model expected for {tier:?}"
            );
        }
    }

    #[test]
    fn registry_ids_are_unique() {
        let r = builtin_registry();
        let mut ids: Vec<&str> = r.iter().map(|m| m.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate model id in registry");
    }

    #[test]
    fn bundled_models_are_permissively_licensed() {
        // Anything shipped inside an offline installer must be redistributable.
        // Gemma and Llama carry custom terms and must never be marked true.
        for m in builtin_registry().iter().filter(|m| m.redistributable) {
            assert!(
                m.license.contains("Apache") || m.license.contains("MIT"),
                "{} claims redistributable under {}",
                m.id,
                m.license
            );
        }
    }

    #[test]
    fn registry_ram_matches_tier_budget() {
        for m in builtin_registry() {
            let budget = m.tier.model_ram_gib();
            assert!(
                (m.ram_gib - budget).abs() < 1.5,
                "{} claims {:.1} GiB but tier {:?} budgets {:.1}",
                m.id,
                m.ram_gib,
                m.tier,
                budget
            );
        }
    }

    #[test]
    fn download_url_is_well_formed() {
        let m = &builtin_registry()[0];
        let url = m.download_url();
        assert!(url.starts_with("https://huggingface.co/"));
        assert!(url.contains(&m.repo));
        assert!(url.contains(&m.file));
    }

    /* ---------- install state ---------- */

    #[test]
    fn reports_not_installed_on_empty_dir() {
        let dir = temp_dir();
        let mm = ModelManager::new(dir.clone());
        let spec = mm.for_tier(Tier::T1).unwrap();
        assert_eq!(mm.state_of(spec), InstallState::NotInstalled);
        assert!(mm.best_installed().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_an_installed_model() {
        let dir = temp_dir();
        let mm = ModelManager::new(dir.clone());
        let spec = mm.for_tier(Tier::T1).unwrap().clone();

        let bytes = (spec.size_gib * 1024.0 * 1024.0 * 1024.0) as usize;
        // Write a sparse file so the test does not cost gigabytes.
        let f = std::fs::File::create(mm.local_path(&spec)).unwrap();
        f.set_len(bytes as u64).unwrap();

        assert_eq!(mm.state_of(&spec), InstallState::Installed);
        assert_eq!(mm.best_installed().map(|s| s.id.clone()), Some(spec.id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flags_truncated_downloads_as_corrupt() {
        let dir = temp_dir();
        let mm = ModelManager::new(dir.clone());
        let spec = mm.for_tier(Tier::T1).unwrap().clone();
        std::fs::write(mm.local_path(&spec), b"not a real model").unwrap();
        assert_eq!(mm.state_of(&spec), InstallState::Corrupt);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn part_file_means_download_in_progress() {
        let dir = temp_dir();
        let mm = ModelManager::new(dir.clone());
        let spec = mm.for_tier(Tier::T1).unwrap().clone();
        std::fs::write(dir.join(format!("{}.part", spec.file)), b"partial").unwrap();
        assert_eq!(mm.state_of(&spec), InstallState::Downloading);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_to_best_installed_when_tier_model_is_missing() {
        let dir = temp_dir();
        let mm = ModelManager::new(dir.clone());

        // Only the T1 model is present, but the machine profiles as T3.
        let t1 = mm.for_tier(Tier::T1).unwrap().clone();
        let f = std::fs::File::create(mm.local_path(&t1)).unwrap();
        f.set_len((t1.size_gib * 1024.0 * 1024.0 * 1024.0) as u64)
            .unwrap();

        let resolved = mm.resolve_for_tier(Tier::T3).unwrap();
        assert_eq!(
            resolved.id, t1.id,
            "should fall back rather than refuse to start"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefers_the_highest_installed_tier() {
        let dir = temp_dir();
        let mm = ModelManager::new(dir.clone());
        for tier in [Tier::T1, Tier::T2] {
            let s = mm.for_tier(tier).unwrap().clone();
            let f = std::fs::File::create(mm.local_path(&s)).unwrap();
            f.set_len((s.size_gib * 1024.0 * 1024.0 * 1024.0) as u64)
                .unwrap();
        }
        assert_eq!(mm.best_installed().unwrap().tier, Tier::T2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_user_supplied_models() {
        let dir = temp_dir();
        let mm = ModelManager::new(dir.clone());
        std::fs::write(dir.join("my-own-model.gguf"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();

        let found = mm.foreign_models();
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("my-own-model.gguf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_override_file_is_honoured() {
        let dir = temp_dir();
        let custom = vec![ModelSpec {
            id: "custom".into(),
            name: "Custom".into(),
            tier: Tier::T1,
            repo: "me/mine".into(),
            file: "custom.gguf".into(),
            size_gib: 1.0,
            ram_gib: 3.0,
            license: "MIT".into(),
            redistributable: true,
            sha256: None,
            context: 8192,
        }];
        std::fs::write(
            dir.join("registry.json"),
            serde_json::to_string(&custom).unwrap(),
        )
        .unwrap();

        let mm = ModelManager::new(dir.clone()).with_registry_file();
        assert_eq!(mm.registry().len(), 1);
        assert_eq!(mm.registry()[0].id, "custom");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_registry_file_falls_back_to_builtin() {
        let dir = temp_dir();
        std::fs::write(dir.join("registry.json"), b"{ not json").unwrap();
        let mm = ModelManager::new(dir.clone()).with_registry_file();
        assert_eq!(mm.registry().len(), builtin_registry().len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_passes_when_no_hash_is_published() {
        let dir = temp_dir();
        let mm = ModelManager::new(dir.clone());
        let spec = mm.for_tier(Tier::T1).unwrap();
        assert!(mm.verify(spec).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_detects_a_wrong_hash() {
        let dir = temp_dir();
        let mut mm = ModelManager::new(dir.clone());
        mm.registry[0].sha256 =
            Some("0000000000000000000000000000000000000000000000000000000000000000".into());
        let spec = mm.registry[0].clone();
        std::fs::write(mm.local_path(&spec), b"abc").unwrap();
        assert!(!mm.verify(&spec).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_accepts_a_correct_hash() {
        let dir = temp_dir();
        let mut mm = ModelManager::new(dir.clone());
        mm.registry[0].sha256 = Some(sha_hex(b"abc"));
        let spec = mm.registry[0].clone();
        std::fs::write(mm.local_path(&spec), b"abc").unwrap();
        assert!(mm.verify(&spec).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
