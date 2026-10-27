//! Single source of truth for model residency.
//!
//! Orion keeps exactly one GGUF resident in RAM at a time — a hard
//! constraint on an 8 GB machine with a 2 GB model ceiling. Every decision
//! about *which* one is therefore load-bearing, and every decision used to
//! be made in a different place with a different copy of the rules. The
//! copies drifted, and the drift shipped:
//!
//! * The persona→model mapping recognised only the literal string
//!   `"developer"`. The Researcher's `DeepSeek-R1-Distill` weights were
//!   downloaded, checksum-verified and then never loaded, because the
//!   mapping keyed on a different spelling than the one the onboarding
//!   wizard wrote to the database.
//! * The swap decision compared *labels* (`"coder"` / `"general"`) that had
//!   desynchronised from the GGUF actually mapped. A cold start in
//!   Developer mode therefore tore down and reloaded a model that was
//!   already correct — a multi-second stall on every launch.
//!
//! Everything that decides model identity now goes through this module:
//! the persona mapping, the router's hand-off, the swap check and the
//! system-panel readout.

use std::path::{Path, PathBuf};

use crate::error::{OrionError, Result};
use crate::models::{InstallState, ModelManager};
use crate::persona::PersonaKind;
use crate::profiler::Tier;

/// Which specialised slot a set of weights belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelRole {
    /// The conversational front door. Serves text and speech.
    General,
    /// Code-specialised weights.
    Coder,
    /// Reasoning-specialised weights.
    Reasoning,
}

impl ModelRole {
    /// Map a persona onto the slot that persona's weights live in.
    ///
    /// This is the mapping that was previously duplicated inline in three
    /// places, one of which only matched the literal string `"developer"`.
    pub fn for_persona(persona: &PersonaKind) -> Self {
        match persona {
            PersonaKind::Developer => Self::Coder,
            PersonaKind::Researcher => Self::Reasoning,
            PersonaKind::Creative | PersonaKind::General => Self::General,
        }
    }

    /// Human-readable name for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Coder => "Coder",
            Self::Reasoning => "Reasoning",
        }
    }

    /// Filename markers that identify a role's dedicated weights.
    ///
    /// The Coder and Reasoning marker sets are deliberately disjoint. A
    /// filename matching both would make the role ambiguous, and the
    /// resolver would then fall back to directory order — non-deterministic
    /// across filesystems, and therefore a source of phantom model swaps.
    fn markers(self) -> &'static [&'static str] {
        match self {
            Self::General => &[],
            Self::Coder => &["coder", "starcoder", "codellama", "codestral"],
            Self::Reasoning => &["deepseek", "r1", "distill", "reasoner", "qwq"],
        }
    }
}

/// The role a GGUF filename belongs to.
///
/// Markers are matched case-insensitively over the ASCII range only: the
/// filename is used as an opaque label, and `to_ascii_lowercase` keeps the
/// byte length stable so nothing downstream can be misaligned.
pub fn role_of_filename(name: &str) -> ModelRole {
    let lower = name.to_ascii_lowercase();
    if ModelRole::Coder.markers().iter().any(|m| lower.contains(m)) {
        return ModelRole::Coder;
    }
    if ModelRole::Reasoning.markers().iter().any(|m| lower.contains(m)) {
        return ModelRole::Reasoning;
    }
    ModelRole::General
}

/// The role a resident weights file belongs to.
pub fn role_of_path(path: &Path) -> ModelRole {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(role_of_filename)
        .unwrap_or(ModelRole::General)
}

/// Every `.gguf` in the models directory that belongs to `role`, sorted.
///
/// Sorting is not cosmetic. The swap check compares resolved *paths*, so
/// the resolver has to return the same file every time it is asked.
/// `read_dir` order is not stable across filesystems, and an unstable
/// resolver manufactures a model swap on every turn.
pub fn role_weights(models: &ModelManager, role: ModelRole) -> Vec<PathBuf> {
    let markers = role.markers();
    if markers.is_empty() {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(models.models_dir()) else {
        return Vec::new();
    };

    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("gguf"))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| {
                    let lower = n.to_ascii_lowercase();
                    markers.iter().any(|m| lower.contains(m))
                })
                .unwrap_or(false)
        })
        .collect();
    out.sort();
    out
}

/// Resolve the weights that should be resident for `role`.
///
/// Priority:
///   1. `ORION_MODEL_PATH` — explicit operator override, always wins
///   2. the role's dedicated weights, if any are installed
///   3. the catalogue model for the active tier
///   4. the best installed catalogue model of any tier
///   5. any user-supplied `.gguf` in the models directory
pub fn resolve_weights(models: &ModelManager, tier: Tier, role: ModelRole) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("ORION_MODEL_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(OrionError::Config(format!(
            "ORION_MODEL_PATH does not point at a file: {}",
            p.display()
        )));
    }

    // Dedicated weights for the requested role.
    if let Some(path) = role_weights(models, role).into_iter().next() {
        return Ok(path);
    }

    // The catalogue model for the active tier.
    if let Some(spec) = models.for_tier(tier) {
        if models.state_of(spec) == InstallState::Installed {
            return Ok(models.local_path(spec));
        }
    }

    // The best installed catalogue model of any tier.
    if let Some(spec) = models.best_installed() {
        return Ok(models.local_path(spec));
    }

    // A user-supplied model we did not install.
    if let Some(found) = models.foreign_models().into_iter().next() {
        return Ok(found);
    }

    Err(OrionError::NoModel)
}

/// Whether a turn asks for programming work.
///
/// The General model is the front door: it handles text and speech and
/// decides whether the turn needs a specialist. The heuristic is
/// deliberately conservative — a false positive costs a 2 GB model swap
/// and a multi-second stall, while a false negative costs answer quality.
pub fn requests_code(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();

    // A fenced code block is a strong signal on its own — but only when the
    // surrounding text is actually about code. A pasted snippet with no
    // context is not a request to swap models.
    if message.contains("```")
        && (lower.contains("code")
            || lower.contains("error")
            || lower.contains("bug")
            || lower.contains("fix")
            || lower.contains("compile")
            || lower.contains("stacktrace")
            || lower.contains("stack trace")
            || lower.contains("refactor"))
    {
        return true;
    }

    const CODE_PREFIXES: &[&str] = &[
        "write code",
        "write a script",
        "write a function",
        "write a python script",
        "write a rust function",
        "write a javascript function",
        "implement a function",
        "create a function",
        "refactor this code",
        "debug this code",
        "fix this code",
        "fix the bug",
        "review this code",
    ];
    CODE_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// The role and effective persona for one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Routing {
    /// The slot whose weights should be resident.
    pub role: ModelRole,
    /// The persona whose weights are loaded. This can differ from the
    /// persona the user selected: the General front door borrows the
    /// Coder slot for a code request, but the user's stored persona is
    /// untouched.
    pub persona: PersonaKind,
}

/// Decide which weights a turn should run against.
///
/// A specialist persona *pins* its slot — the Researcher's reasoning
/// weights stay resident regardless of the turn's subject, because the
/// user explicitly chose that specialisation during onboarding. The
/// General front door may *borrow* the Coder slot.
pub fn resolve_routing(persona: PersonaKind, message: &str) -> Routing {
    match persona {
        PersonaKind::Developer => Routing {
            role: ModelRole::Coder,
            persona: PersonaKind::Developer,
        },
        PersonaKind::Researcher => Routing {
            role: ModelRole::Reasoning,
            persona: PersonaKind::Researcher,
        },
        PersonaKind::Creative | PersonaKind::General => {
            if requests_code(message) {
                Routing {
                    role: ModelRole::Coder,
                    persona: PersonaKind::Developer,
                }
            } else {
                Routing {
                    role: ModelRole::General,
                    persona: PersonaKind::General,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orion-residency-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn manager_with(files: &[&str]) -> (PathBuf, ModelManager) {
        let dir = scratch_dir("weights");
        for f in files {
            std::fs::write(dir.join(f), b"gguf").expect("write fixture");
        }
        let m = ModelManager::new(dir.clone());
        (dir, m)
    }

    /* ---------- persona mapping ---------- */

    #[test]
    fn developer_persona_maps_to_the_coder_slot() {
        assert_eq!(
            ModelRole::for_persona(&PersonaKind::Developer),
            ModelRole::Coder
        );
    }

    #[test]
    fn researcher_persona_maps_to_the_reasoning_slot() {
        // This mapping is the one that shipped broken: the Researcher only
        // ever matched the literal string "developer", so the R1 weights
        // were downloaded and never loaded.
        assert_eq!(
            ModelRole::for_persona(&PersonaKind::Researcher),
            ModelRole::Reasoning
        );
    }

    #[test]
    fn creative_and_general_personas_map_to_the_general_slot() {
        assert_eq!(
            ModelRole::for_persona(&PersonaKind::Creative),
            ModelRole::General
        );
        assert_eq!(
            ModelRole::for_persona(&PersonaKind::General),
            ModelRole::General
        );
    }

    #[test]
    fn every_persona_maps_to_exactly_one_role() {
        for p in [
            PersonaKind::Developer,
            PersonaKind::Researcher,
            PersonaKind::Creative,
            PersonaKind::General,
        ] {
            // Mapping twice must agree.
            assert_eq!(ModelRole::for_persona(&p), ModelRole::for_persona(&p));
            let _ = ModelRole::for_persona(&p).label();
        }
    }

    /* ---------- markers ---------- */

    #[test]
    fn coder_and_reasoning_markers_are_disjoint() {
        // A filename matching both sets would make the role ambiguous.
        for c in ModelRole::Coder.markers() {
            for r in ModelRole::Reasoning.markers() {
                assert!(!c.contains(r), "coder marker '{c}' contains '{r}'");
                assert!(!r.contains(c), "reasoning marker '{r}' contains '{c}'");
            }
        }
    }

    #[test]
    fn a_coder_filename_is_recognised() {
        assert_eq!(
            role_of_filename("qwen2.5-coder-3b-instruct-q4_k_m.gguf"),
            ModelRole::Coder
        );
    }

    #[test]
    fn a_reasoning_filename_is_recognised() {
        assert_eq!(
            role_of_filename("deepseek-r1-distill-qwen-7b-q4_k_m.gguf"),
            ModelRole::Reasoning
        );
    }

    #[test]
    fn a_general_filename_is_recognised() {
        assert_eq!(
            role_of_filename("qwen2.5-3b-instruct-q4_k_m.gguf"),
            ModelRole::General
        );
    }

    #[test]
    fn marker_matching_is_case_insensitive() {
        assert_eq!(
            role_of_filename("Qwen2.5-CODER-3B-Instruct.gguf"),
            ModelRole::Coder
        );
        assert_eq!(
            role_of_filename("DeepSeek-R1-Distill.gguf"),
            ModelRole::Reasoning
        );
    }

    /* ---------- weight discovery ---------- */

    #[test]
    fn weight_discovery_is_deterministic() {
        let (dir, m) = manager_with(&[
            "qwen2.5-coder-7b-instruct-q4_k_m.gguf",
            "qwen2.5-coder-3b-instruct-q4_k_m.gguf",
            "deepseek-r1-distill-qwen-7b-q4_k_m.gguf",
        ]);

        let first = role_weights(&m, ModelRole::Coder);
        let second = role_weights(&m, ModelRole::Coder);
        assert_eq!(first, second, "resolver must be stable across calls");
        assert_eq!(first.len(), 2);
        // Lexicographic order: the 3B file sorts before the 7B file.
        assert!(first[0].to_string_lossy().contains("3b"));
        assert!(first[1].to_string_lossy().contains("7b"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn weight_discovery_ignores_non_gguf_files() {
        let (dir, m) = manager_with(&["qwen2.5-coder-3b.gguf", "qwen2.5-coder-3b.gguf.json"]);
        assert_eq!(role_weights(&m, ModelRole::Coder).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_general_role_has_no_dedicated_weights() {
        let (dir, m) = manager_with(&["qwen2.5-3b-instruct-q4_k_m.gguf"]);
        assert!(role_weights(&m, ModelRole::General).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /* ---------- routing ---------- */

    #[test]
    fn code_requests_are_detected() {
        assert!(requests_code("write code to sort a list"));
        assert!(requests_code("implement a function that reverses a string"));
        assert!(requests_code("debug this code for me"));
        // A fenced block only counts when the surrounding text is about
        // code; a pasted snippet with no context is not a request to swap
        // a 2 GB model into RAM.
        assert!(requests_code(
            "here is my code, it has a bug:\n```rust\nfn main() {}\n```"
        ));
    }

    #[test]
    fn prose_questions_are_not_code_requests() {
        assert!(!requests_code("what is the capital of france"));
        assert!(!requests_code("explain the borrow checker"));
        assert!(!requests_code("summarise the news about mars"));
        assert!(!requests_code("what does this do?\n```\nprint(1)\n```"));
    }

    #[test]
    fn the_router_borrows_the_coder_slot_for_code() {
        let r = resolve_routing(PersonaKind::General, "write code to parse a csv");
        assert_eq!(r.role, ModelRole::Coder);
        assert_eq!(r.persona, PersonaKind::Developer);
    }

    #[test]
    fn a_specialist_persona_pins_its_slot() {
        // The Researcher's reasoning weights stay resident whatever the
        // turn is about: the user chose that specialisation deliberately.
        let r = resolve_routing(PersonaKind::Researcher, "what is the capital of france");
        assert_eq!(r.role, ModelRole::Reasoning);
        assert_eq!(r.persona, PersonaKind::Researcher);

        let d = resolve_routing(PersonaKind::Developer, "what is the capital of france");
        assert_eq!(d.role, ModelRole::Coder);
        assert_eq!(d.persona, PersonaKind::Developer);
    }

    #[test]
    fn the_router_keeps_the_general_slot_for_ordinary_questions() {
        let r = resolve_routing(PersonaKind::General, "what is the capital of france");
        assert_eq!(r.role, ModelRole::General);
        assert_eq!(r.persona, PersonaKind::General);
    }

    /* ---------- resident reporting ---------- */

    #[test]
    fn the_resident_role_is_inferred_from_the_resident_file() {
        assert_eq!(
            role_of_path(Path::new("/models/qwen2.5-coder-3b-instruct-q4_k_m.gguf")),
            ModelRole::Coder
        );
        assert_eq!(
            role_of_path(Path::new("/models/deepseek-r1-distill.gguf")),
            ModelRole::Reasoning
        );
        assert_eq!(
            role_of_path(Path::new("/models/qwen2.5-3b-instruct-q4_k_m.gguf")),
            ModelRole::General
        );
    }
}
