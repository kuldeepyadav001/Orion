import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

function formatBytes(bytes) {
  if (!bytes || bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

export default function DocumentCard({ doc }) {
  const [opening, setOpening] = useState(false);
  const [showing, setShowing] = useState(false);
  const [errorMsg, setErrorMsg] = useState(null);

  if (!doc) return null;

  const ext = (doc.file_type || doc.filename?.split(".").pop() || "").toLowerCase();

  const config = {
    xlsx: {
      label: "Excel Spreadsheet",
      icon: "📊",
      badgeClass: "badge-excel",
      bgClass: "card-excel",
      openTitle: "Open in Excel / LibreOffice",
    },
    docx: {
      label: "Word Document",
      icon: "📄",
      badgeClass: "badge-word",
      bgClass: "card-word",
      openTitle: "Open in Microsoft Word",
    },
    pdf: {
      label: "PDF Report",
      icon: "📑",
      badgeClass: "badge-pdf",
      bgClass: "card-pdf",
      openTitle: "Open in PDF Viewer",
    },
  }[ext] || {
    label: "Document",
    icon: "📁",
    badgeClass: "badge-default",
    bgClass: "card-default",
    openTitle: "Open File",
  };

  const handleOpen = async () => {
    if (!doc.path) return;
    setOpening(true);
    setErrorMsg(null);
    try {
      await invoke("open_document", { path: doc.path });
    } catch (err) {
      console.error("Failed to open document:", err);
      setErrorMsg("Could not launch default app");
      setTimeout(() => setErrorMsg(null), 3000);
    } finally {
      setOpening(false);
    }
  };

  const handleShowInFolder = async () => {
    if (!doc.path) return;
    setShowing(true);
    setErrorMsg(null);
    try {
      await invoke("show_in_folder", { path: doc.path });
    } catch (err) {
      console.error("Failed to show in folder:", err);
      setErrorMsg("Could not open file explorer");
      setTimeout(() => setErrorMsg(null), 3000);
    } finally {
      setShowing(false);
    }
  };

  const handleDownload = () => {
    if (!doc.base64 && !doc.path) return;

    if (doc.base64) {
      const mime =
        ext === "xlsx"
          ? "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
          : ext === "docx"
            ? "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            : "application/pdf";

      const byteCharacters = atob(doc.base64);
      const byteNumbers = Array.from({ length: byteCharacters.length });
      for (let i = 0; i < byteCharacters.length; i++) {
        byteNumbers[i] = byteCharacters.charCodeAt(i);
      }
      const byteArray = new Uint8Array(byteNumbers);
      const blob = new Blob([byteArray], { type: mime });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = doc.filename || `document.${ext}`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } else {
      handleShowInFolder();
    }
  };

  return (
    <div className={`generated-doc-card ${config.bgClass}`}>
      <div className="doc-card-top">
        <div className="doc-card-icon">{config.icon}</div>
        <div className="doc-card-info">
          <div className="doc-card-filename" title={doc.path || doc.filename}>
            {doc.filename}
          </div>
          <div className="doc-card-meta">
            <span className={`doc-card-badge ${config.badgeClass}`}>{config.label}</span>
            <span className="doc-card-size">{formatBytes(doc.size_bytes)}</span>
            <span className="doc-card-status">Saved locally</span>
          </div>
        </div>
      </div>

      {errorMsg && <div className="doc-card-error">{errorMsg}</div>}

      <div className="doc-card-actions">
        <button
          type="button"
          className="btn-doc-open"
          onClick={handleOpen}
          disabled={opening}
          title={config.openTitle}
        >
          {opening ? "Opening…" : "📂 Open File"}
        </button>
        <button
          type="button"
          className="btn-doc-folder"
          onClick={handleShowInFolder}
          disabled={showing}
          title="Show in File Explorer / Folder"
        >
          {showing ? "Opening…" : "🔍 Show in Folder"}
        </button>
        <button
          type="button"
          className="btn-doc-download"
          onClick={handleDownload}
          title="Save / Download Copy"
        >
          💾 Download Copy
        </button>
      </div>
    </div>
  );
}
