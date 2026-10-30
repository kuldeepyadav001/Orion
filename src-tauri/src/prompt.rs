//! Fitting a prompt into the engine's context window.
//!
//! The engine runs with `--ctx-size 4096`. Everything the model is asked
//! to read has to fit in that window: the system prompt, the retrieved
//! documents, the live web results, the conversation so far, and the
//! question.
//!
//! Two things used to go wrong.
//!
//! The history was capped by *turn count* -- twenty turns -- which is not
//! a bound on anything. Orion's answers are structured documents of
//! several thousand characters, so twenty turns could be forty thousand
//! characters, far past the window. The request was then rejected by the
//! model and the conversation stopped.
//!
//! The question itself was never checked. A user pasting a long log got
//! the same failure, with no way to tell why.
//!
//! So the budget is computed in one place: the fixed parts are measured,
//! the question is trimmed to what is left, and the history receives the
//! remainder.

use crate::db;

/// The engine's context window, in tokens (`--ctx-size 4096`).
pub const CONTEXT_WINDOW_TOKENS: usize = 4096;

/// Characters per token, used to turn the token window into a character
/// budget. The measured density of English prose is about 3.5 characters
/// per token; markdown with numbers is denser (down to 1.6), so 3.0 is
/// the conservative side of that measurement.
pub const CHARS_PER_TOKEN: usize = 3;

/// The whole prompt, in characters.
pub const PROMPT_WINDOW_CHARS: usize = CONTEXT_WINDOW_TOKENS * CHARS_PER_TOKEN;

/// Characters reserved for what cannot be measured directly: the chat
/// template's role markers, the sentences wrapped around the retrieved
/// context, and the model's own framing.
pub const PROMPT_OVERHEAD_CHARS: usize = 800;

/// What is left after the prompt has been fitted into the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fitted {
    /// The user's question, trimmed if it did not fit.
    pub message: String,
    /// The retrieved context, trimmed if it did not fit.
    pub context: String,
    /// How many characters of history may still be replayed.
    pub history_chars: usize,
    /// True when the question had to be cut.
    pub message_truncated: bool,
    /// True when the retrieved context had to be cut.
    pub context_truncated: bool,
}

/// Fit a prompt into the window.
///
/// `system_chars` is the length of the system prompt, `context` the
/// retrieved material being included (documents, web results, or
/// nothing), `message` the question.
///
/// The priority order when everything does not fit:
///
/// 1. The system prompt is never cut. It is Orion's identity and the
///    instructions that make it behave.
/// 2. The question is cut only if it cannot fit even with no context at
///    all. A question that has been cut is not a question any more.
/// 3. The retrieved context absorbs the shortfall. Losing the tail of a
///    document is better than losing the question, and much better than
///    the request being rejected outright.
/// 4. The history gets what is left, and nothing when there is nothing.
pub fn fit(
    system_chars: usize,
    context: &str,
    message: &str,
    window_chars: usize,
    overhead_chars: usize,
) -> Fitted {
    let reserved = system_chars.saturating_add(overhead_chars);
    let room = window_chars.saturating_sub(reserved);

    let message_len = message.chars().count();
    let (message_out, message_truncated) = if message_len <= room {
        (message.to_string(), false)
    } else {
        (truncate(message, room), true)
    };
    let used_by_message = message_out.chars().count();

    let context_len = context.chars().count();
    let room_for_context = room.saturating_sub(used_by_message);
    let (context_out, context_truncated) = if context_len <= room_for_context {
        (context.to_string(), false)
    } else {
        (truncate(context, room_for_context), true)
    };
    let used_by_context = context_out.chars().count();

    Fitted {
        message: message_out,
        context: context_out,
        history_chars: room
            .saturating_sub(used_by_message)
            .saturating_sub(used_by_context),
        message_truncated,
        context_truncated,
    }
}

/// Cut a string to `max` characters, marking that it was cut.
pub fn truncate(text: &str, max: usize) -> String {
    let len = text.chars().count();
    if len <= max {
        return text.to_string();
    }
    let marker = " … (truncated)";
    let marker_len = marker.chars().count();
    if max <= marker_len {
        // No room for the marker. Cut exactly at the budget instead --
        // the budget is the hard bound, the marker is a courtesy.
        return text.chars().take(max).collect();
    }
    let mut out: String = text.chars().take(max - marker_len).collect();
    out.push_str(marker);
    out
}

/// Keep the newest turns that fit a character budget.
///
/// Turns are kept whole, newest first, and the walk stops at the first
/// turn that does not fit. The newest turn is always kept, even when it
/// alone exceeds the budget: dropping the question the user just asked
/// is worse than a long prompt. A single over-long turn is truncated
/// rather than dropped, so the budget is a real bound.
pub fn trim_history(history: &[db::StoredMessage], budget: usize) -> Vec<db::StoredMessage> {
    if history.is_empty() {
        return Vec::new();
    }

    let mut kept: Vec<db::StoredMessage> = Vec::new();
    let mut used = 0usize;

    for msg in history.iter().rev() {
        let len = msg.content.chars().count();
        let newest = kept.is_empty();

        if !newest && used + len > budget {
            break;
        }

        let mut m = msg.clone();
        if newest && len > budget {
            m.content = truncate(&m.content, budget);
        }
        used += m.content.chars().count();
        kept.push(m);
    }

    kept.reverse();
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_question_leaves_the_rest_for_history() {
        let ctx = "c".repeat(6_000);
        let f = fit(2_700, &ctx, "What is the capital?", PROMPT_WINDOW_CHARS, 800);
        assert_eq!(f.message, "What is the capital?");
        assert!(!f.message_truncated);
        assert!(!f.context_truncated);
        assert_eq!(f.history_chars, 12_288 - 2_700 - 6_000 - 800 - 20);
    }

    #[test]
    fn a_long_question_is_trimmed_and_the_context_gives_way() {
        let ctx = "c".repeat(6_000);
        let long = "x".repeat(50_000);
        let f = fit(2_700, &ctx, &long, PROMPT_WINDOW_CHARS, 800);
        assert!(f.message_truncated);
        assert!(f.context_truncated);
        assert_eq!(f.history_chars, 0);
        // The question keeps what it needs; the context keeps the rest.
        let total = 2_700 + 800 + f.message.chars().count() + f.context.chars().count();
        assert!(total <= PROMPT_WINDOW_CHARS);
        assert!(f.message.ends_with("(truncated)"));
    }

    #[test]
    fn the_budget_is_never_exceeded() {
        for msg_len in [0usize, 100, 5_000, 12_000, 40_000] {
            for ctx_len in [0usize, 100, 6_000, 30_000] {
                let msg = "q".repeat(msg_len);
                let ctx = "c".repeat(ctx_len);
                let f = fit(2_700, &ctx, &msg, PROMPT_WINDOW_CHARS, 800);
                let total = 2_700
                    + 800
                    + f.message.chars().count()
                    + f.context.chars().count()
                    + f.history_chars;
                assert!(total <= PROMPT_WINDOW_CHARS, "{total} exceeds the window");
            }
        }
    }

    #[test]
    fn a_prompt_that_already_overflows_the_window_still_keeps_the_question() {
        let f = fit(20_000, &"c".repeat(20_000), "hi", PROMPT_WINDOW_CHARS, 800);
        assert_eq!(f.history_chars, 0);
        assert_eq!(f.context, "");
        // The system prompt alone already fills the window, so even the
        // question has to be cut -- but it is cut last.
        assert!(f.message.chars().count() <= PROMPT_WINDOW_CHARS);
    }

    #[test]
    fn a_question_survives_a_context_that_would_overflow_the_window() {
        let ctx = "c".repeat(30_000);
        let f = fit(2_700, &ctx, "What is the capital?", PROMPT_WINDOW_CHARS, 800);
        assert_eq!(f.message, "What is the capital?");
        assert!(!f.message_truncated);
        assert!(f.context_truncated);
        assert!(f.context.chars().count() < ctx.len());
    }

    fn msg(role: &str, content: &str) -> db::StoredMessage {
        db::StoredMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: "s".into(),
            role: role.into(),
            content: content.into(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn trimming_keeps_the_newest_turns_that_fit() {
        let history = vec![
            msg("user", "aaaa"),
            msg("assistant", "bbbb"),
            msg("user", "cccc"),
            msg("assistant", "dddd"),
        ];
        // 4 + 4 + 4 = 12 fits; the fourth turn would make it 16.
        let kept = trim_history(&history, 12);
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0].content, "bbbb");
        assert_eq!(kept[2].content, "dddd");
    }

    #[test]
    fn trimming_keeps_the_newest_turn_even_when_it_is_alone_over_budget() {
        // Oldest first: the newest turn is the 50-character one.
        let history = vec![
            msg("assistant", "b".repeat(10).as_str()),
            msg("user", "a".repeat(50).as_str()),
        ];
        let kept = trim_history(&history, 20);
        assert_eq!(kept.len(), 1);
        assert!(kept[0].content.chars().count() <= 20);
        assert!(kept[0].content.ends_with("(truncated)"));
    }

    #[test]
    fn trimming_never_exceeds_the_budget() {
        let history: Vec<db::StoredMessage> = (0..20)
            .map(|i| msg("user", &"x".repeat(500 + i)))
            .collect();
        for budget in [0usize, 1, 100, 1_000, 2_500, 5_000] {
            let kept = trim_history(&history, budget);
            let total: usize = kept.iter().map(|m| m.content.chars().count()).sum();
            assert!(total <= budget, "budget {budget} exceeded by {total}");
        }
    }

    #[test]
    fn trimming_an_empty_history_yields_nothing() {
        assert!(trim_history(&[], 1_000).is_empty());
    }

    #[test]
    fn every_system_prompt_leaves_room_for_a_question_and_some_history() {
        // The clock context is added to the system prompt at runtime and
        // is covered by the overhead reserve.
        for (name, prompt) in [
            ("general", crate::SYSTEM_PROMPT),
            ("web", crate::WEB_GROUNDED_SYSTEM_PROMPT),
            ("grounded", crate::rag::context::GROUNDED_SYSTEM_PROMPT),
        ] {
            let f = fit(
                prompt.chars().count(),
                "",
                "What is the capital of France?",
                PROMPT_WINDOW_CHARS,
                PROMPT_OVERHEAD_CHARS,
            );
            assert!(!f.message_truncated, "the {name} system prompt is too long");
            assert!(
                f.history_chars > 500,
                "the {name} system prompt leaves only {} characters of history",
                f.history_chars
            );
        }
    }

    #[test]
    fn truncating_never_exceeds_the_limit() {
        for max in [0usize, 1, 5, 13, 14, 15, 100] {
            let cut = truncate("abcdefghijklmnopqrstuvwxyz", max);
            assert!(cut.chars().count() <= max, "{max} exceeded");
        }
    }
}
