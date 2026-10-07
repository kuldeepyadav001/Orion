import { useState, useMemo, useRef, useEffect } from "react";

export function formatRelativeTime(dateString) {
  if (!dateString) return "";
  const date = new Date(dateString);
  const now = new Date();
  const diffMs = now - date;
  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHour = Math.floor(diffMin / 60);
  const diffDay = Math.floor(diffHour / 24);

  if (diffSec < 60) return "Just now";
  if (diffMin < 60) return `${diffMin}m ago`;
  if (diffHour < 24) return `${diffHour}h ago`;
  if (diffDay === 1) return "Yesterday";
  if (diffDay < 7) return `${diffDay}d ago`;
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function getGroupForDate(dateString) {
  if (!dateString) return "Older";
  const date = new Date(dateString);
  const now = new Date();
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const startOfYesterday = new Date(startOfToday.getTime() - 24 * 60 * 60 * 1000);
  const startOf7DaysAgo = new Date(startOfToday.getTime() - 7 * 24 * 60 * 60 * 1000);

  if (date >= startOfToday) return "Today";
  if (date >= startOfYesterday) return "Yesterday";
  if (date >= startOf7DaysAgo) return "Previous 7 Days";
  return "Older";
}

/**
 * Storage-First Chat History List:
 * Displays conversations grouped chronologically with search, inline rename,
 * and deletion confirmation. Only loads metadata into memory.
 */
export function ChatHistoryList({
  sessions = [],
  activeSessionId,
  onSelectSession,
  onCreateSession,
  onDeleteSession,
  onRenameSession,
}) {
  const [searchQuery, setSearchQuery] = useState("");
  const [editingId, setEditingId] = useState(null);
  const [editTitle, setEditTitle] = useState("");
  const [pendingDeleteId, setPendingDeleteId] = useState(null);
  const editInputRef = useRef(null);

  useEffect(() => {
    if (editingId && editInputRef.current) {
      editInputRef.current.focus();
      editInputRef.current.select();
    }
  }, [editingId]);

  const filteredSessions = useMemo(() => {
    if (!searchQuery.trim()) return sessions;
    const q = searchQuery.toLowerCase();
    return sessions.filter(
      (s) =>
        s.title.toLowerCase().includes(q) ||
        (s.snippet && s.snippet.toLowerCase().includes(q))
    );
  }, [sessions, searchQuery]);

  const groupedSessions = useMemo(() => {
    const groups = {
      Today: [],
      Yesterday: [],
      "Previous 7 Days": [],
      Older: [],
    };

    for (const session of filteredSessions) {
      const group = getGroupForDate(session.updated_at);
      if (groups[group]) {
        groups[group].push(session);
      } else {
        groups.Older.push(session);
      }
    }

    return Object.entries(groups).filter(([, items]) => items.length > 0);
  }, [filteredSessions]);

  const handleStartRename = (session, e) => {
    e.stopPropagation();
    setEditingId(session.id);
    setEditTitle(session.title);
  };

  const handleSaveRename = (sessionId) => {
    if (editTitle.trim() && editTitle.trim() !== "") {
      onRenameSession(sessionId, editTitle.trim());
    }
    setEditingId(null);
  };

  const handleKeyDownRename = (e, sessionId) => {
    if (e.key === "Enter") {
      handleSaveRename(sessionId);
    } else if (e.key === "Escape") {
      setEditingId(null);
    }
  };

  return (
    <div className="chat-history-container">
      {/* Search Input */}
      {sessions.length > 3 && (
        <div className="chat-history-search-wrap">
          <input
            type="text"
            placeholder="Search chats..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="chat-history-search-input"
          />
          {searchQuery && (
            <button
              type="button"
              className="chat-search-clear"
              onClick={() => setSearchQuery("")}
            >
              ✕
            </button>
          )}
        </div>
      )}

      {/* List */}
      <div className="chat-history-scroll-area">
        {sessions.length === 0 ? (
          <div className="chat-history-empty">
            <p>No saved conversations</p>
            <button
              type="button"
              className="btn-create-first"
              onClick={onCreateSession}
            >
              Start new chat
            </button>
          </div>
        ) : filteredSessions.length === 0 ? (
          <div className="chat-history-no-matches">
            <p>No chats match "{searchQuery}"</p>
          </div>
        ) : (
          groupedSessions.map(([groupName, items]) => (
            <div key={groupName} className="chat-date-group">
              <div className="chat-date-group-title">{groupName}</div>
              <div className="chat-session-list">
                {items.map((session) => {
                  const isActive = session.id === activeSessionId;
                  const isEditing = editingId === session.id;

                  return (
                    <div
                      key={session.id}
                      className={`chat-session-item ${isActive ? "active" : ""}`}
                      onClick={() => {
                        if (!isEditing) {
                          onSelectSession(session.id);
                        }
                      }}
                      title={session.title}
                    >
                      <div className="chat-session-item-content">
                        {isEditing ? (
                          <input
                            ref={editInputRef}
                            type="text"
                            value={editTitle}
                            onChange={(e) => setEditTitle(e.target.value)}
                            onBlur={() => handleSaveRename(session.id)}
                            onKeyDown={(e) => handleKeyDownRename(e, session.id)}
                            onClick={(e) => e.stopPropagation()}
                            className="chat-session-rename-input"
                            maxLength={60}
                          />
                        ) : (
                          <>
                            <div className="chat-session-title-line">
                              <span className="chat-session-title">
                                {session.title || "New chat"}
                              </span>
                              <span className="chat-session-time">
                                {formatRelativeTime(session.updated_at)}
                              </span>
                            </div>
                            {session.snippet && (
                              <div className="chat-session-snippet">
                                {session.snippet}
                              </div>
                            )}
                          </>
                        )}
                      </div>

                      {/* Action buttons (Rename & Delete) */}
                      {!isEditing && (
                        <div
                          className="chat-session-actions"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <button
                            type="button"
                            className="chat-action-btn rename"
                            onClick={(e) => handleStartRename(session, e)}
                            title="Rename"
                            aria-label="Rename chat"
                          >
                            ✏️
                          </button>
                          <button
                            type="button"
                            className="chat-action-btn delete"
                            onClick={(e) => {
                              e.stopPropagation();
                              setPendingDeleteId(session.id);
                            }}
                            title="Delete"
                            aria-label="Delete chat"
                          >
                            🗑️
                          </button>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          ))
        )}
      </div>

      {/* Delete Confirmation Modal */}
      {pendingDeleteId && (
        <div
          className="delete-modal-backdrop"
          onClick={() => setPendingDeleteId(null)}
        >
          <div
            className="delete-modal-dialog"
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
          >
            <div className="delete-modal-header">
              <span className="delete-warning-icon">⚠️</span>
              <h3>Delete Conversation?</h3>
            </div>
            <p className="delete-modal-body">
              This will permanently delete this conversation and all associated
              messages from your local SQLite database.
            </p>
            <div className="delete-modal-actions">
              <button
                type="button"
                className="delete-cancel-btn"
                onClick={() => setPendingDeleteId(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                className="delete-confirm-btn"
                onClick={() => {
                  onDeleteSession(pendingDeleteId);
                  setPendingDeleteId(null);
                }}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default function ChatSidebar({
  isOpen,
  onClose,
  sessions = [],
  activeSessionId,
  onSelectSession,
  onCreateSession,
  onDeleteSession,
  onRenameSession,
}) {
  return (
    <>
      {isOpen && (
        <div
          className="chat-sidebar-backdrop"
          onClick={onClose}
          aria-hidden="true"
        />
      )}
      <aside
        className={`chat-sidebar ${isOpen ? "open" : ""}`}
        aria-label="Conversation History"
      >
        <div className="chat-sidebar-header">
          <div className="chat-sidebar-title-row">
            <span className="chat-sidebar-heading">Conversations</span>
            <button
              type="button"
              className="chat-sidebar-close-btn"
              onClick={onClose}
              title="Close sidebar"
            >
              ✕
            </button>
          </div>
          <button
            type="button"
            className="chat-sidebar-new-btn"
            onClick={onCreateSession}
            title="Create new conversation (Ctrl+N)"
          >
            <span className="new-chat-icon">+</span>
            <span className="new-chat-text">New Chat</span>
            <kbd className="new-chat-kbd">Ctrl+N</kbd>
          </button>
        </div>

        <ChatHistoryList
          sessions={sessions}
          activeSessionId={activeSessionId}
          onSelectSession={onSelectSession}
          onCreateSession={onCreateSession}
          onDeleteSession={onDeleteSession}
          onRenameSession={onRenameSession}
        />

        <div className="chat-sidebar-footer">
          <div className="chat-storage-badge">
            <span className="storage-dot"></span>
            <span className="storage-text">
              SQLite Storage • {sessions.length} conversation{sessions.length === 1 ? "" : "s"}
            </span>
          </div>
        </div>
      </aside>
    </>
  );
}
