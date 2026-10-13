import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IconMail,
  IconTrash,
  IconRotateCw,
  IconCopy,
  IconCheck,
  IconZap,
  IconExternalLink,
  IconX,
  IconPlus,
} from "./Icons";

export default function EmailStudio({ isOpen, onClose }) {
  const [emails, setEmails] = useState([]);
  const [selectedEmailId, setSelectedEmailId] = useState(null);
  const [selectedEmail, setSelectedEmail] = useState(null);
  const [filter, setFilter] = useState("all"); // "all" | "unread"
  const [loading, setLoading] = useState(false);
  const [replyInput, setReplyInput] = useState("");
  const [draftText, setDraftText] = useState("");
  const [drafting, setDrafting] = useState(false);
  const [copied, setCopied] = useState(false);

  // Connect IMAP modal state
  const [showConnectModal, setShowConnectModal] = useState(false);
  const [imapHost, setImapHost] = useState("imap.gmail.com");
  const [imapPort, setImapPort] = useState(993);
  const [imapUser, setImapUser] = useState("");
  const [imapPass, setImapPass] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [statusNote, setStatusNote] = useState(null);

  const loadEmails = useCallback(async (activeFilter) => {
    setLoading(true);
    try {
      const filterArg = activeFilter === "all" ? null : activeFilter;
      const list = await invoke("list_inbox_emails", { filter: filterArg });
      setEmails(list || []);
      if (list && list.length > 0 && !selectedEmailId) {
        setSelectedEmailId(list[0].id);
      }
    } catch {
      // Mock / standalone preview fallback
      setEmails([
        {
          id: "m1",
          sender: "alex.turner@vanguard.io",
          sender_name: "Alex Turner",
          subject: "Q4 Infrastructure Expansion & Budget Review",
          body_snippet: "Hi Team, review the attached budget forecast for Q4 before Friday's call...",
          received_at: new Date().toISOString(),
          is_read: true,
          has_draft: false,
        },
        {
          id: "m2",
          sender: "sarah.chen@innovate.co",
          sender_name: "Sarah Chen",
          subject: "Project Timeline Update & API Handshake",
          body_snippet: "We completed the client SDK integration. Could you confirm if the endpoint...",
          received_at: new Date(Date.now() - 3600000 * 2).toISOString(),
          is_read: false,
          has_draft: true,
        },
        {
          id: "m3",
          sender: "newsletter@mit.edu",
          sender_name: "MIT Tech Digest",
          subject: "Weekly AI Frontiers: Autonomous Agents on Edge Hardware",
          body_snippet: "In this week's issue: New benchmarks for edge models under 4B parameters...",
          received_at: new Date(Date.now() - 3600000 * 24).toISOString(),
          is_read: true,
          has_draft: false,
        },
      ]);
      if (!selectedEmailId) setSelectedEmailId("m1");
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
      return;
    }
    invoke("get_inbox_email", { id: selectedEmailId })
      .then((email) => {
        setSelectedEmail(email);
        setDraftText(email?.draft_reply || "");
        if (!email?.is_read) {
          invoke("mark_inbox_email_read", { id: email.id, isRead: true }).catch(() => {});
          setEmails((prev) =>
            prev.map((e) => (e.id === email.id ? { ...e, is_read: true } : e))
          );
        }
      })
      .catch(() => {
        const found = emails.find((e) => e.id === selectedEmailId);
        if (found) {
          setSelectedEmail({
            ...found,
            recipients: "me@orion.local",
            body_clean: found.body_snippet + "\n\nPlease let me know your thoughts so we can finalize the schedule.\n\nBest regards,\n" + (found.sender_name || "Sender"),
          });
          setDraftText("");
        }
      });
  }, [selectedEmailId, emails]);

  const handleGenerateReply = async () => {
    if (!selectedEmail) return;
    setDrafting(true);
    try {
      const promptDirective = replyInput.trim()
        ? `Respond addressing this directive: ${replyInput.trim()}`
        : "Acknowledge receipt professionally and confirm next steps.";
      const res = await invoke("draft_email_response", {
        emailId: selectedEmail.id,
        tone: "professional",
        customDirective: promptDirective,
      });
      setDraftText(res.draft);
      setReplyInput("");
    } catch {
      // Clean fallback generation
      const senderFirst = (selectedEmail.sender_name || "there").split(" ")[0];
      setDraftText(
        `Hi ${senderFirst},\n\nThank you for reaching out regarding "${selectedEmail.subject}".\n\nI have reviewed the details and will proceed with the requested items. I'll keep you updated on progress.\n\nBest regards,\nSent via Orion`
      );
      setReplyInput("");
    } finally {
      setDrafting(false);
    }
  };

  const handleCopy = () => {
    if (!draftText) return;
    navigator.clipboard.writeText(draftText);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const handleDelete = async () => {
    if (!selectedEmailId) return;
    try {
      await invoke("delete_inbox_email", { id: selectedEmailId });
    } catch {
      /* ignore */
    }
    setEmails((prev) => prev.filter((e) => e.id !== selectedEmailId));
    setSelectedEmailId(null);
    setSelectedEmail(null);
  };

  const handleConnectImap = async (e) => {
    e.preventDefault();
    setConnecting(true);
    setStatusNote(null);
    try {
      const count = await invoke("sync_user_inbox", {
        imapHost,
        imapPort: Number(imapPort),
        username: imapUser,
        authToken: imapPass,
        limit: 20,
      });
      setStatusNote(`Connected! Synced ${count} emails.`);
      setShowConnectModal(false);
      loadEmails(filter);
    } catch (err) {
      setStatusNote(`Connection failed: ${err}`);
    } finally {
      setConnecting(false);
    }
  };

  if (!isOpen) return null;

  const displayEmails = filter === "unread" ? emails.filter((e) => !e.is_read) : emails;

  return (
    <div className="email-modal-backdrop" onClick={onClose}>
      <div
        className="email-modal-dialog simple-email-dialog"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        {/* Sleek Minimal Header */}
        <div className="email-modal-header">
          <div className="email-header-title-wrap">
            <div className="email-header-icon-box">
              <IconMail size={17} />
            </div>
            <div>
              <h3>Email</h3>
              <p className="email-header-sub">
                Private offline inbox &amp; instant AI reply assistant
              </p>
            </div>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
            <button
              type="button"
              className="email-secondary-btn"
              onClick={() => setShowConnectModal(true)}
              title="Connect Gmail or IMAP account"
            >
              <IconPlus size={13} />
              <span>Connect Account</span>
            </button>
            <button
              type="button"
              className="email-refresh-btn"
              onClick={() => loadEmails(filter)}
              title="Refresh inbox"
            >
              <IconRotateCw size={13} className={loading ? "spin" : ""} />
              <span>Refresh</span>
            </button>
            <button
              type="button"
              className="email-close-btn"
              onClick={onClose}
              title="Close (Esc)"
              aria-label="Close"
            >
              <IconX size={15} />
            </button>
          </div>
        </div>

        {/* Filter bar */}
        <div className="email-simple-filter-bar">
          <div className="email-filter-pills">
            <button
              type="button"
              className={`email-filter-btn ${filter === "all" ? "active" : ""}`}
              onClick={() => setFilter("all")}
            >
              All Messages ({emails.length})
            </button>
            <button
              type="button"
              className={`email-filter-btn ${filter === "unread" ? "active" : ""}`}
              onClick={() => setFilter("unread")}
            >
              Unread ({emails.filter((e) => !e.is_read).length})
            </button>
          </div>

          {statusNote && <div className="email-status-text">{statusNote}</div>}
        </div>

        {/* Split Inbox View */}
        <div className="email-split-container">
          {/* Left: Clean Message List */}
          <div className="email-list-pane simple-list-pane">
            {displayEmails.length === 0 && (
              <div className="email-pane-empty">No emails in this view.</div>
            )}
            {displayEmails.map((e) => {
              const isSelected = e.id === selectedEmailId;
              return (
                <div
                  key={e.id}
                  className={`email-simple-item ${isSelected ? "selected" : ""} ${
                    !e.is_read ? "unread" : ""
                  }`}
                  onClick={() => setSelectedEmailId(e.id)}
                >
                  <div className="email-item-top">
                    <span className="email-item-sender">
                      {!e.is_read && <span className="email-unread-dot" />}
                      {e.sender_name || e.sender}
                    </span>
                    <span className="email-item-time">
                      {new Date(e.received_at).toLocaleDateString([], {
                        month: "short",
                        day: "numeric",
                      })}
                    </span>
                  </div>
                  <div className="email-simple-subject">{e.subject}</div>
                  <div className="email-simple-snippet">{e.body_snippet}</div>
                </div>
              );
            })}
          </div>

          {/* Right: Message Reader & Simple Reply */}
          <div className="email-detail-pane simple-detail-pane">
            {selectedEmail ? (
              <div className="email-simple-detail-scroll">
                <div className="email-simple-header">
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
                    <h2 className="email-simple-subject-title">{selectedEmail.subject}</h2>
                    <button
                      type="button"
                      className="email-action-icon-btn delete"
                      onClick={handleDelete}
                      title="Delete email"
                    >
                      <IconTrash size={14} />
                    </button>
                  </div>
                  <div className="email-simple-meta">
                    <span>
                      From: <strong>{selectedEmail.sender_name}</strong> &lt;{selectedEmail.sender}&gt;
                    </span>
                    <span style={{ marginLeft: "12px", color: "#64748b" }}>
                      {new Date(selectedEmail.received_at).toLocaleString()}
                    </span>
                  </div>
                </div>

                <div className="email-simple-body">
                  <pre className="email-body-text">{selectedEmail.body_clean || selectedEmail.body_snippet}</pre>
                </div>

                {/* Clean Reply Box */}
                <div className="email-reply-container">
                  <div className="email-reply-header">
                    <span className="email-reply-title">Reply with Orion</span>
                    {draftText && (
                      <button
                        type="button"
                        className="email-btn-copy-draft"
                        onClick={handleCopy}
                        title="Copy to clipboard"
                      >
                        {copied ? (
                          <>
                            <IconCheck size={12} className="text-emerald" />
                            <span>Copied</span>
                          </>
                        ) : (
                          <>
                            <IconCopy size={12} />
                            <span>Copy Draft</span>
                          </>
                        )}
                      </button>
                    )}
                  </div>

                  {draftText ? (
                    <div className="email-draft-box">
                      <textarea
                        className="email-draft-textarea"
                        value={draftText}
                        onChange={(e) => setDraftText(e.target.value)}
                        rows={5}
                      />
                      <div className="email-draft-actions">
                        <button
                          type="button"
                          className="email-btn-open-client"
                          onClick={() => {
                            const mailto = `mailto:${encodeURIComponent(
                              selectedEmail.sender
                            )}?subject=Re:%20${encodeURIComponent(
                              selectedEmail.subject
                            )}&body=${encodeURIComponent(draftText)}`;
                            window.open(mailto, "_blank");
                          }}
                        >
                          <IconExternalLink size={12} />
                          <span>Open in Mail App</span>
                        </button>
                        <button
                          type="button"
                          className="email-btn-regen"
                          onClick={handleGenerateReply}
                          disabled={drafting}
                        >
                          <IconZap size={12} />
                          <span>{drafting ? "Regenerating..." : "Regenerate"}</span>
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className="email-reply-input-wrap">
                      <input
                        type="text"
                        className="email-reply-input"
                        placeholder="Optional instructions (e.g. 'say I will attend on Friday at 2pm')..."
                        value={replyInput}
                        onChange={(e) => setReplyInput(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" && !e.shiftKey) {
                            e.preventDefault();
                            handleGenerateReply();
                          }
                        }}
                      />
                      <button
                        type="button"
                        className="email-btn-generate"
                        onClick={handleGenerateReply}
                        disabled={drafting}
                      >
                        <IconZap size={12} />
                        <span>{drafting ? "Writing..." : "Write Reply"}</span>
                      </button>
                    </div>
                  )}
                </div>
              </div>
            ) : (
              <div className="email-pane-empty">Select a message to read.</div>
            )}
          </div>
        </div>

        {/* Connect IMAP Modal */}
        {showConnectModal && (
          <div className="email-submodal-backdrop" onClick={() => setShowConnectModal(false)}>
            <div className="email-submodal" onClick={(e) => e.stopPropagation()}>
              <div className="email-submodal-header">
                <h4>Connect Mail Account</h4>
                <button
                  type="button"
                  className="email-close-btn"
                  onClick={() => setShowConnectModal(false)}
                >
                  <IconX size={14} />
                </button>
              </div>

              <form onSubmit={handleConnectImap} className="email-submodal-form">
                <div className="email-form-group">
                  <label>Email Address</label>
                  <input
                    type="email"
                    value={imapUser}
                    onChange={(e) => setImapUser(e.target.value)}
                    placeholder="you@gmail.com"
                    required
                  />
                </div>

                <div className="email-form-group">
                  <label>Google App Password / Token</label>
                  <input
                    type="password"
                    value={imapPass}
                    onChange={(e) => setImapPass(e.target.value)}
                    placeholder="16-character App Password"
                    required
                  />
                  <small style={{ color: "#64748b", fontSize: "11px", marginTop: "3px" }}>
                    For Gmail, use a 16-letter App Password from myaccount.google.com/apppasswords.
                  </small>
                </div>

                <div style={{ display: "grid", gridTemplateColumns: "2fr 1fr", gap: "10px" }}>
                  <div className="email-form-group">
                    <label>IMAP Host</label>
                    <input
                      type="text"
                      value={imapHost}
                      onChange={(e) => setImapHost(e.target.value)}
                      required
                    />
                  </div>
                  <div className="email-form-group">
                    <label>Port</label>
                    <input
                      type="number"
                      value={imapPort}
                      onChange={(e) => setImapPort(e.target.value)}
                      required
                    />
                  </div>
                </div>

                <div style={{ display: "flex", justifyContent: "flex-end", gap: "8px", marginTop: "12px" }}>
                  <button
                    type="button"
                    className="email-secondary-btn"
                    onClick={() => setShowConnectModal(false)}
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    className="email-btn-generate"
                    disabled={connecting}
                  >
                    {connecting ? "Connecting..." : "Sync Inbox"}
                  </button>
                </div>
              </form>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
