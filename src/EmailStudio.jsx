import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IconMail,
  IconShield,
  IconTrash,
  IconRotateCw,
  IconCopy,
  IconCheck,
  IconZap,
  IconExternalLink,
  IconX,
  IconLock,
  IconPencil,
} from "./Icons";

export default function EmailStudio({ isOpen, onClose }) {
  const [emails, setEmails] = useState([]);
  const [selectedEmailId, setSelectedEmailId] = useState(null);
  const [selectedEmail, setSelectedEmail] = useState(null);
  const [filter, setFilter] = useState("all");
  const [loading, setLoading] = useState(false);
  const [draftTone, setDraftTone] = useState("professional");
  const [draftText, setDraftText] = useState("");
  const [drafting, setDrafting] = useState(false);
  const [copyFeedback, setCopyFeedback] = useState(false);
  const [errorMsg, setErrorMsg] = useState(null);

  const loadEmails = useCallback(async (activeFilter) => {
    setLoading(true);
    setErrorMsg(null);
    try {
      const filterArg = activeFilter === "all" ? null : activeFilter;
      const list = await invoke("list_inbox_emails", { filter: filterArg });
      setEmails(list || []);
      if (list && list.length > 0 && !selectedEmailId) {
        setSelectedEmailId(list[0].id);
      }
    } catch (err) {
      console.error("Failed to load emails:", err);
      setErrorMsg("Failed to query local inbox.");
    } finally {
      setLoading(false);
    }
  }, [selectedEmailId]);

  useEffect(() => {
    if (isOpen) {
      loadEmails(filter);
    }
  }, [isOpen, filter, loadEmails]);

  useEffect(() => {
    if (!selectedEmailId) {
      setSelectedEmail(null);
      setDraftText("");
      return;
    }

    let active = true;
    invoke("get_inbox_email", { id: selectedEmailId })
      .then((email) => {
        if (!active) return;
        setSelectedEmail(email);
        setDraftText(email.draft_reply || "");
        if (!email.is_read) {
          invoke("mark_inbox_email_read", { id: email.id, isRead: true })
            .then(() => {
              setEmails((prev) =>
                prev.map((e) => (e.id === email.id ? { ...e, is_read: true } : e))
              );
            })
            .catch(() => {});
        }
      })
      .catch((err) => {
        console.error("Failed to get email details:", err);
      });

    return () => {
      active = false;
    };
  }, [selectedEmailId]);

  const handleGenerateDraft = async () => {
    if (!selectedEmail) return;
    setDrafting(true);
    try {
      const generated = await invoke("draft_email_response", {
        id: selectedEmail.id,
        tone: draftTone,
        userName: "Orion User",
      });
      setDraftText(generated);
      setSelectedEmail((prev) => (prev ? { ...prev, draft_reply: generated } : prev));
      setEmails((prev) =>
        prev.map((e) => (e.id === selectedEmail.id ? { ...e, has_draft: true } : e))
      );
    } catch (err) {
      console.error("Failed to draft reply:", err);
      setErrorMsg("Failed to generate draft reply.");
      setTimeout(() => setErrorMsg(null), 3000);
    } finally {
      setDrafting(false);
    }
  };

  const handleCopyDraft = async () => {
    if (!draftText) return;
    try {
      await navigator.clipboard.writeText(draftText);
      setCopyFeedback(true);
      setTimeout(() => setCopyFeedback(false), 2000);
    } catch (err) {
      console.error("Failed to copy draft:", err);
    }
  };

  const handleOpenInMailClient = () => {
    if (!selectedEmail) return;
    const cleanSub = selectedEmail.subject.toLowerCase().startsWith("re:")
      ? selectedEmail.subject
      : `Re: ${selectedEmail.subject}`;
    const mailto = `mailto:${encodeURIComponent(selectedEmail.sender)}?subject=${encodeURIComponent(
      cleanSub
    )}&body=${encodeURIComponent(draftText || "")}`;
    window.open(mailto, "_blank");
  };

  const handleDelete = async () => {
    if (!selectedEmailId) return;
    try {
      await invoke("delete_inbox_email", { id: selectedEmailId });
      setEmails((prev) => prev.filter((e) => e.id !== selectedEmailId));
      setSelectedEmailId(null);
      setSelectedEmail(null);
    } catch (err) {
      console.error("Failed to delete email:", err);
    }
  };

  if (!isOpen) return null;

  let parsedActionItems = [];
  if (selectedEmail?.action_items) {
    try {
      parsedActionItems = JSON.parse(selectedEmail.action_items);
    } catch {
      parsedActionItems = [];
    }
  }

  return (
    <div className="email-modal-backdrop" onClick={onClose}>
      <div
        className="email-modal-dialog"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        {/* Top Header */}
        <div className="email-modal-header">
          <div className="email-header-title-wrap">
            <div className="email-header-icon-box">
              <IconMail size={18} />
            </div>
            <div>
              <h3>Sovereign Email Assistant</h3>
              <p className="email-header-sub">
                Rule R-5: Strictly No Auto-Send • Untrusted Domain Multi-Vector Sanitized
              </p>
            </div>
          </div>
          <button
            type="button"
            className="email-close-btn"
            onClick={onClose}
            title="Close Email Assistant (Esc)"
            aria-label="Close"
          >
            <IconX size={15} />
          </button>
        </div>

        {/* Filter Navigation */}
        <div className="email-filter-bar">
          <div className="email-filter-pills">
            <button
              type="button"
              className={`email-filter-btn ${filter === "all" ? "active" : ""}`}
              onClick={() => setFilter("all")}
            >
              All Inbox ({emails.length})
            </button>
            <button
              type="button"
              className={`email-filter-btn urgent ${filter === "urgent" ? "active" : ""}`}
              onClick={() => setFilter("urgent")}
            >
              <span className="filter-dot urgent-dot" />
              <span>Urgent</span>
            </button>
            <button
              type="button"
              className={`email-filter-btn action ${filter === "action" ? "active" : ""}`}
              onClick={() => setFilter("action")}
            >
              <span className="filter-dot action-dot" />
              <span>Action Required</span>
            </button>
            <button
              type="button"
              className={`email-filter-btn newsletter ${filter === "newsletter" ? "active" : ""}`}
              onClick={() => setFilter("newsletter")}
            >
              <span className="filter-dot newsletter-dot" />
              <span>Newsletter</span>
            </button>
            <button
              type="button"
              className={`email-filter-btn spam ${filter === "spam" ? "active" : ""}`}
              onClick={() => setFilter("spam")}
            >
              <span className="filter-dot spam-dot" />
              <span>Quarantined</span>
            </button>
          </div>
          <button
            type="button"
            className="email-refresh-btn"
            onClick={() => loadEmails(filter)}
            title="Sync local inbox"
          >
            <IconRotateCw size={13} className={loading ? "spin" : ""} />
            <span>Sync</span>
          </button>
        </div>

        {errorMsg && <div className="email-error-bar">{errorMsg}</div>}

        {/* Main Split View */}
        <div className="email-split-container">
          {/* Left: Email List */}
          <div className="email-list-pane">
            {loading && <div className="email-pane-status">Refreshing local messages…</div>}
            {!loading && emails.length === 0 && (
              <div className="email-pane-empty">No emails match this filter.</div>
            )}
            {emails.map((e) => {
              const isSelected = e.id === selectedEmailId;
              const catClass =
                e.triage_category === "Urgent"
                  ? "cat-urgent"
                  : e.triage_category === "Action Required"
                    ? "cat-action"
                    : e.triage_category === "Newsletter"
                      ? "cat-newsletter"
                      : e.triage_category === "Spam / Suspicious"
                        ? "cat-quarantine"
                        : "cat-info";

              return (
                <div
                  key={e.id}
                  className={`email-item ${isSelected ? "selected" : ""} ${
                    !e.is_read ? "unread" : ""
                  }`}
                  onClick={() => setSelectedEmailId(e.id)}
                >
                  <div className="email-item-top">
                    <span className="email-item-sender">{e.sender_name || e.sender}</span>
                    <span className="email-item-time">
                      {new Date(e.received_at).toLocaleTimeString([], {
                        hour: "2-digit",
                        minute: "2-digit",
                      })}
                    </span>
                  </div>
                  <div className="email-item-subject">{e.subject}</div>
                  <div className="email-item-snippet">{e.body_snippet}</div>
                  <div className="email-item-meta">
                    <span className={`email-badge ${catClass}`}>{e.triage_category}</span>
                    <span className="email-priority-tag">P{e.priority_score}</span>
                    {e.has_draft && (
                      <span className="email-draft-pill" title="Draft reply ready">
                        <IconPencil size={11} />
                        <span>Draft ready</span>
                      </span>
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          {/* Right: Reading & Drafting Pane */}
          <div className="email-detail-pane">
            {selectedEmail ? (
              <div className="email-detail-scroll">
                {/* Quarantine / Security Banner */}
                <div className="email-security-banner">
                  <span className="security-icon">
                    <IconShield size={16} />
                  </span>
                  <div className="security-text">
                    <strong>Untrusted Domain Quarantine Active</strong> • Zero-width characters &amp;
                    embedded scripts stripped • Non-egress execution
                  </div>
                </div>

                {/* Email Header */}
                <div className="email-view-header">
                  <div className="email-view-top-row">
                    <h2 className="email-view-subject">{selectedEmail.subject}</h2>
                    <div className="email-view-actions">
                      <button
                        type="button"
                        className="email-action-icon-btn delete"
                        onClick={handleDelete}
                        title="Delete message"
                        aria-label="Delete message"
                      >
                        <IconTrash size={14} />
                      </button>
                    </div>
                  </div>
                  <div className="email-view-addresses">
                    <div>
                      <strong>From:</strong> {selectedEmail.sender_name} &lt;{selectedEmail.sender}&gt;
                    </div>
                    <div>
                      <strong>To:</strong> {selectedEmail.recipients}
                    </div>
                    <div className="email-view-date">
                      {new Date(selectedEmail.received_at).toLocaleString()}
                    </div>
                  </div>
                </div>

                {/* Triage Insight Box */}
                <div className="email-triage-box">
                  <div className="triage-title">
                    <span className="triage-title-text">
                      <IconZap size={14} />
                      <span>Orion Sovereign Triage</span>
                    </span>
                    <span className="triage-cat-badge">{selectedEmail.triage_category}</span>
                  </div>
                  {selectedEmail.triage_reason && (
                    <div className="triage-reason">{selectedEmail.triage_reason}</div>
                  )}
                  {parsedActionItems.length > 0 && (
                    <div className="triage-actions-list">
                      <div className="triage-actions-label">Extracted Action Items:</div>
                      <ul>
                        {parsedActionItems.map((act, idx) => (
                          <li key={idx}>
                            <IconCheck size={12} className="action-check-icon" />
                            <span>{act}</span>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                </div>

                {/* Sanitized Message Body */}
                <div className="email-body-container">
                  <div className="email-body-heading">Sanitized Message Body</div>
                  <div className="email-sanitized-text">{selectedEmail.body_sanitized}</div>
                </div>

                {/* Sovereign Draft Studio */}
                <div className="email-draft-studio">
                  <div className="draft-studio-header">
                    <div className="draft-studio-title">
                      <IconPencil size={14} />
                      <span>Contextual Draft Reply</span>
                      <span className="rule-badge">Rule R-5: No Auto-Send</span>
                    </div>

                    <div className="draft-tone-selector">
                      <button
                        type="button"
                        className={`tone-btn ${draftTone === "professional" ? "active" : ""}`}
                        onClick={() => setDraftTone("professional")}
                      >
                        Professional
                      </button>
                      <button
                        type="button"
                        className={`tone-btn ${draftTone === "direct" ? "active" : ""}`}
                        onClick={() => setDraftTone("direct")}
                      >
                        Direct
                      </button>
                      <button
                        type="button"
                        className={`tone-btn ${draftTone === "technical" ? "active" : ""}`}
                        onClick={() => setDraftTone("technical")}
                      >
                        Technical
                      </button>
                      <button
                        type="button"
                        className={`tone-btn ${draftTone === "polite" ? "active" : ""}`}
                        onClick={() => setDraftTone("polite")}
                      >
                        Polite
                      </button>
                    </div>
                  </div>

                  <div className="draft-generate-row">
                    <button
                      type="button"
                      className="btn-generate-draft"
                      onClick={handleGenerateDraft}
                      disabled={drafting}
                    >
                      <IconZap size={13} />
                      <span>{drafting ? "Synthesizing Sovereign Draft…" : "Generate Sovereign Draft"}</span>
                    </button>
                  </div>

                  <textarea
                    className="draft-textarea"
                    rows={7}
                    placeholder="Click 'Generate Sovereign Draft' or compose your response here..."
                    value={draftText}
                    onChange={(e) => setDraftText(e.target.value)}
                  />

                  <div className="draft-actions-bar">
                    <div className="draft-guard-note">
                      <IconLock size={12} />
                      <span>Draft is stored locally. You retain 100% control over outbound transmission.</span>
                    </div>
                    <div className="draft-btns">
                      <button
                        type="button"
                        className="btn-copy-draft"
                        onClick={handleCopyDraft}
                        disabled={!draftText}
                      >
                        {copyFeedback ? (
                          <>
                            <IconCheck size={13} />
                            <span>Copied!</span>
                          </>
                        ) : (
                          <>
                            <IconCopy size={13} />
                            <span>Copy Draft</span>
                          </>
                        )}
                      </button>
                      <button
                        type="button"
                        className="btn-launch-client"
                        onClick={handleOpenInMailClient}
                        disabled={!draftText}
                        title="Open system default mail client (mailto:)"
                      >
                        <IconExternalLink size={13} />
                        <span>Open in Mail Client</span>
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            ) : (
              <div className="email-pane-unselected">
                <div className="empty-mail-box">
                  <IconMail size={28} />
                </div>
                <h3>Select an email to inspect</h3>
                <p>
                  Inbound messages are isolated in the Untrusted Domain. Orion automatically triages
                  deadlines and drafts context-aware responses without external egress.
                </p>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
