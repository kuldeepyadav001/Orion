//! Workload personas and specialization engine (M6).
//!
//! ## Why Personas
//!
//! A single generic system prompt cannot produce expert-grade software
//! architecture, rigorous multi-document research, and compelling creative prose
//! simultaneously — especially on 3B quantized models operating within a 5.7 GB
//! usable RAM budget.
//!
//! Personas tailor Orion's tone, structure, reasoning depth, and syntax guidelines
//! to the user's immediate professional task without requiring larger models
//! or simultaneous dual-model loading in memory.

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{OrionError, Result};

pub const SETTING_KEY_PERSONA: &str = "active_persona";
pub const SETTING_KEY_ONBOARDING: &str = "onboarding_completed";

/// Enumeration of all available specialized workload personas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonaKind {
    /// Software developer, architect, systems engineer.
    Developer,
    /// Researcher, scientist, data analyst.
    Researcher,
    /// Creative writer, worldbuilder, narrative designer.
    Creative,
    /// Balanced general conversational assistant (default).
    General,
}

impl PersonaKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "developer" | "coder" | "dev" => Some(Self::Developer),
            "researcher" | "analyst" | "research" => Some(Self::Researcher),
            "creative" | "writer" | "narrative" => Some(Self::Creative),
            "general" | "assistant" | "default" => Some(Self::General),
            _ => None,
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::Developer => "developer",
            Self::Researcher => "researcher",
            Self::Creative => "creative",
            Self::General => "general",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Developer => "Software Developer",
            Self::Researcher => "Researcher & Analyst",
            Self::Creative => "Creative Writer",
            Self::General => "General Assistant",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Developer => "💻",
            Self::Researcher => "🔬",
            Self::Creative => "🎨",
            Self::General => "⚡",
        }
    }

    pub fn tagline(&self) -> &'static str {
        match self {
            Self::Developer => "Architectural precision, idiomatic syntax, zero boilerplate",
            Self::Researcher => "Deep citation synthesis, analytical rigor, evidence-backed",
            Self::Creative => "Engaging narrative, expressive dialogue, vivid worldbuilding",
            Self::General => "Direct, concise, balanced everyday personal intelligence",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Developer => {
                "Optimized for writing clean, secure code, refactoring architectures, \
                 debugging complex systems, and explaining algorithmic trade-offs."
            }
            Self::Researcher => {
                "Optimized for synthesizing insights across technical documents, \
                 structuring citations, identifying methodological gaps, and critical analysis."
            }
            Self::Creative => {
                "Optimized for crafting stories, worldbuilding, screenwriting, dialogue, \
                 and descriptive prompt generation with rich sensory detail."
            }
            Self::General => {
                "Balanced for daily productivity, quick answers, drafting emails, \
                 and multi-purpose conversation with fast turnaround."
            }
        }
    }

    /// Additional behavioral directives prepended or appended to the base system prompt.
    pub fn system_directive(&self) -> &'static str {
        match self {
            Self::Developer => {
                "Persona: Senior Software Architect & Engineer.\n\
                 - Write idiomatic, robust, typed code with minimal conversational fluff.\n\
                 - Prioritize memory safety, edge cases, error handling, and algorithmic complexity.\n\
                 - Adhere strictly to the requested programming language's modern standards.\n\
                 - If a solution requires architectural trade-offs, state them concisely."
            }
            Self::Researcher => {
                "Persona: Analytical Researcher & Technical Synthesizer.\n\
                 - Structure answers with clear analytical frameworks, bulleted evidence, and summaries.\n\
                 - Distinguish verified empirical facts from hypotheses or extrapolations.\n\
                 - If document sources are available, attribute insights directly to the evidence.\n\
                 - Avoid speculative leaps; explicitly state data limitations."
            }
            Self::Creative => {
                "Persona: Creative Storyteller & Narrative Designer.\n\
                 - Use vivid, sensory language, dynamic pacing, and expressive dialogue.\n\
                 - Build engaging scenarios, flesh out authentic character voices, and show rather than tell.\n\
                 - Avoid generic or repetitive tropes unless intentionally subverting them."
            }
            Self::General => {
                "Persona: Direct, helpful personal assistant.\n\
                 - Provide clear, accurate, and concise answers.\n\
                 - Be helpful, respectful, and transparent about capabilities."
            }
        }
    }

    /// Integrate the persona directive into the active system prompt.
    pub fn enhance_prompt(&self, base_prompt: &str) -> String {
        format!("{}\n\n{}", base_prompt, self.system_directive())
    }

    pub fn info(&self) -> PersonaInfo {
        PersonaInfo {
            id: self.id().to_string(),
            name: self.name().to_string(),
            icon: self.icon().to_string(),
            tagline: self.tagline().to_string(),
            description: self.description().to_string(),
        }
    }
}

/// Information packet serializable to the frontend UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaInfo {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub tagline: String,
    pub description: String,
}

/// Return metadata for all available personas.
pub fn list_personas() -> Vec<PersonaInfo> {
    vec![
        PersonaKind::General.info(),
        PersonaKind::Developer.info(),
        PersonaKind::Researcher.info(),
        PersonaKind::Creative.info(),
    ]
}

/// Retrieve the active persona from the database settings, defaulting to General.
pub fn get_active_persona(db: &Db) -> Result<PersonaKind> {
    match db.get_setting(SETTING_KEY_PERSONA)? {
        Some(val) => Ok(PersonaKind::parse(&val).unwrap_or(PersonaKind::General)),
        None => Ok(PersonaKind::General),
    }
}

/// Update the active persona in the database settings.
pub fn set_active_persona(db: &Db, persona_id: &str) -> Result<PersonaInfo> {
    let kind = PersonaKind::parse(persona_id)
        .ok_or_else(|| OrionError::Config(format!("unknown persona: '{persona_id}'")))?;

    db.set_setting(SETTING_KEY_PERSONA, kind.id())?;
    tracing::info!(persona = kind.id(), "active workload persona updated");
    Ok(kind.info())
}

/* ------------------------------------------------------------------ */
/* master passcode security lock                                      */
/* ------------------------------------------------------------------ */

pub const SETTING_KEY_LOCK_ENABLED: &str = "lock_enabled";
pub const SETTING_KEY_LOCK_HASH: &str = "lock_hash";
pub const SETTING_KEY_LOCK_HINT: &str = "lock_hint";
const LOCK_SALT: &str = "orion-local-passcode-salt-v1";

/// Compute a secure salted SHA-256 hash of the master passcode.
pub fn hash_passcode(passcode: &str) -> String {
    let salted = format!("{passcode}:{LOCK_SALT}");
    crate::hashing::sha256_hex(salted.as_bytes())
}

/// Check if master passcode security is configured in settings.
pub fn is_lock_configured(db: &Db) -> Result<bool> {
    match db.get_setting(SETTING_KEY_LOCK_ENABLED)? {
        Some(v) => Ok(v == "true" || v == "1"),
        None => Ok(false),
    }
}

/// Verify an input passcode against the stored salted hash.
pub fn verify_passcode(db: &Db, passcode: &str) -> Result<bool> {
    if !is_lock_configured(db)? {
        return Ok(true); // No lock configured; access granted
    }
    match db.get_setting(SETTING_KEY_LOCK_HASH)? {
        Some(stored_hash) => {
            let candidate_hash = hash_passcode(passcode);
            Ok(candidate_hash == stored_hash)
        }
        None => Ok(true),
    }
}

/// Configure or update the master security passcode.
pub fn set_passcode(db: &Db, passcode: &str, hint: Option<&str>) -> Result<()> {
    if passcode.trim().len() < 4 {
        return Err(OrionError::Config(
            "master passcode must be at least 4 characters".into(),
        ));
    }
    let hashed = hash_passcode(passcode.trim());
    db.set_setting(SETTING_KEY_LOCK_HASH, &hashed)?;
    db.set_setting(SETTING_KEY_LOCK_ENABLED, "true")?;
    if let Some(h) = hint {
        db.set_setting(SETTING_KEY_LOCK_HINT, h)?;
    }
    tracing::info!("master security passcode configured");
    Ok(())
}

/// Remove master passcode security after verifying the current passcode.
pub fn remove_passcode(db: &Db, current_passcode: &str) -> Result<bool> {
    if !verify_passcode(db, current_passcode)? {
        return Ok(false);
    }
    db.set_setting(SETTING_KEY_LOCK_ENABLED, "false")?;
    tracing::info!("master security passcode disabled");
    Ok(true)
}

/// Read the password hint, if one was provided.
pub fn get_lock_hint(db: &Db) -> Result<Option<String>> {
    db.get_setting(SETTING_KEY_LOCK_HINT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_persona_variations() {
        assert_eq!(PersonaKind::parse("developer"), Some(PersonaKind::Developer));
        assert_eq!(PersonaKind::parse("coder"), Some(PersonaKind::Developer));
        assert_eq!(PersonaKind::parse("dev"), Some(PersonaKind::Developer));

        assert_eq!(PersonaKind::parse("researcher"), Some(PersonaKind::Researcher));
        assert_eq!(PersonaKind::parse("analyst"), Some(PersonaKind::Researcher));

        assert_eq!(PersonaKind::parse("creative"), Some(PersonaKind::Creative));
        assert_eq!(PersonaKind::parse("writer"), Some(PersonaKind::Creative));

        assert_eq!(PersonaKind::parse("general"), Some(PersonaKind::General));
        assert_eq!(PersonaKind::parse("unknown_xyz"), None);
    }

    #[test]
    fn enhances_system_prompt_without_dropping_base() {
        let base = "Base system prompt.";
        let dev = PersonaKind::Developer;
        let enhanced = dev.enhance_prompt(base);
        assert!(enhanced.starts_with(base));
        assert!(enhanced.contains("Software Architect"));
    }

    #[test]
    fn lists_all_four_personas() {
        let all = list_personas();
        assert_eq!(all.len(), 4);
        let ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"general"));
        assert!(ids.contains(&"developer"));
        assert!(ids.contains(&"researcher"));
        assert!(ids.contains(&"creative"));
    }

    #[test]
    fn passcode_lock_lifecycle() {
        let tmp = std::env::temp_dir().join(format!("orion-lock-test-{}", uuid::Uuid::new_v4()));
        let db = Db::open(&tmp.join("test.db")).unwrap();

        assert!(!is_lock_configured(&db).unwrap());
        assert!(verify_passcode(&db, "anything").unwrap());

        // Set passcode
        set_passcode(&db, "secret123", Some("my secret")).unwrap();
        assert!(is_lock_configured(&db).unwrap());
        assert_eq!(get_lock_hint(&db).unwrap(), Some("my secret".into()));

        // Verification
        assert!(verify_passcode(&db, "secret123").unwrap());
        assert!(!verify_passcode(&db, "wrong").unwrap());

        // Remove passcode
        assert!(!remove_passcode(&db, "wrong").unwrap());
        assert!(remove_passcode(&db, "secret123").unwrap());
        assert!(!is_lock_configured(&db).unwrap());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
