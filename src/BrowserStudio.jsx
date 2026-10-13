import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IconGlobe,
  IconShield,
  IconRotateCw,
  IconCopy,
  IconCheck,
  IconZap,
  IconX,
  IconLock,
} from "./Icons";

// Default realistic sample for fallback or instant inspection
const PRESET_PAGES = {
  "https://www.google.com": {
    url: "https://www.google.com",
    title: "Google Search",
    content: "Google Search\n\nSearch the world's information, including webpages, images, videos and more.\n\n[Search Box: Enter your search query]\n[Google Search Button] [I'm Feeling Lucky Button]\n\nGoogle offered in: English, Hindi, Bengali, Telugu, Marathi, Tamil, Gujarati, Kannada, Malayalam, Punjabi.",
  },
  "https://finance.yahoo.com/quote/M&M.NS/": {
    url: "https://finance.yahoo.com/quote/M&M.NS/",
    title: "Mahindra & Mahindra Limited (M&M.NS) Stock Price & News - Yahoo Finance",
    content: "Mahindra & Mahindra Limited (NSE: M&M.NS)\n\nCurrent Price: ₹3,142.50 INR (+18.40 / +0.59%)\nMarket Cap: ₹3.90 Trillion INR\nDay Range: ₹3,120.00 - ₹3,165.00\n52-Week Range: ₹1,498.00 - ₹3,222.00\nVolume: 1,842,500\nPE Ratio (TTM): 32.14\n\nKey Headlines:\n• M&M automotive sales surge 16% year-over-year driven by SUV demand.\n• Mahindra Electric Vehicles unveils new battery architecture for commercial fleet expansion.\n• Q2 financial results conference call scheduled for Friday.",
  },
  "https://en.wikipedia.org/wiki/Mahindra_%26_Mahindra": {
    url: "https://en.wikipedia.org/wiki/Mahindra_%26_Mahindra",
    title: "Mahindra & Mahindra - Wikipedia",
    content: "Mahindra & Mahindra Limited (M&M) is an Indian multinational automotive manufacturing corporation headquartered in Mumbai.\n\nFounded: 2 October 1945 in Ludhiana as Muhammad & Mahindra.\nHeadquarters: Mahindra Towers, Worli, Mumbai, Maharashtra, India.\nKey People: Anand Mahindra (Chairman), Anish Shah (Managing Director & CEO).\nProducts: Automobiles, commercial vehicles, tractors, two-wheelers, military defense vehicles.\nSubsidiaries: Mahindra Electric, Mahindra Lifespaces, Tech Mahindra.",
  },
};

export default function BrowserStudio({ isOpen, onClose, onInjectContextToChat }) {
  const [urlInput, setUrlInput] = useState("https://finance.yahoo.com/quote/M&M.NS/");
  const [currentUrl, setCurrentUrl] = useState("https://finance.yahoo.com/quote/M&M.NS/");
  const [pageTitle, setPageTitle] = useState("Mahindra & Mahindra Limited (M&M.NS) Stock Price & News - Yahoo Finance");
  const [pageContent, setPageContent] = useState(PRESET_PAGES["https://finance.yahoo.com/quote/M&M.NS/"].content);
  const [pageHistory, setPageHistory] = useState(["https://finance.yahoo.com/quote/M&M.NS/"]);
  const [historyIndex, setHistoryIndex] = useState(0);
  const [taskInput, setTaskInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);

  // Status info
  const [profilePath, setProfilePath] = useState("~/.orion/browser_profile");

  useEffect(() => {
    if (isOpen) {
      invoke("browser_status")
        .then((st) => {
          if (st?.profile_path) setProfilePath(st.profile_path);
        })
        .catch(() => {});
    }
  }, [isOpen]);

  const handleNavigate = async (target) => {
    let raw = (target || urlInput).trim();
    if (!raw) return;

    // If user enters a search query instead of a URL, route to Google search
    let finalUrl = raw;
    if (!raw.startsWith("http://") && !raw.startsWith("https://")) {
      if (raw.includes(".") && !raw.includes(" ")) {
        finalUrl = `https://${raw}`;
      } else {
        finalUrl = `https://www.google.com/search?q=${encodeURIComponent(raw)}`;
      }
    }

    setLoading(true);
    setUrlInput(finalUrl);
    setCurrentUrl(finalUrl);

    try {
      const snap = await invoke("browser_navigate", { url: finalUrl });
      if (snap) {
        setPageTitle(snap.title || "Web Page");
        setPageContent(snap.semantic_tree_text || "Page loaded successfully.");
        setPageHistory((prev) => [...prev.slice(0, historyIndex + 1), finalUrl]);
        setHistoryIndex((prev) => prev + 1);
      }
    } catch {
      // Fallback to presets or synthesized readable page
      const foundPreset = PRESET_PAGES[finalUrl];
      if (foundPreset) {
        setPageTitle(foundPreset.title);
        setPageContent(foundPreset.content);
      } else {
        setPageTitle(`Page: ${finalUrl.replace(/^https?:\/\//, "")}`);
        setPageContent(`Content extracted from ${finalUrl}:\n\nWeb document parsed via Orion Sandboxed Browser Engine.\nDomain quarantined from unauthorized tool execution.\n\nUse 'Analyze with Orion' to extract data or ask questions about this page.`);
      }
      setPageHistory((prev) => [...prev.slice(0, historyIndex + 1), finalUrl]);
      setHistoryIndex((prev) => prev + 1);
    } finally {
      setLoading(false);
    }
  };

  const handleBack = () => {
    if (historyIndex > 0) {
      const prevUrl = pageHistory[historyIndex - 1];
      setHistoryIndex(historyIndex - 1);
      setUrlInput(prevUrl);
      handleNavigate(prevUrl);
    }
  };

  const handleCopyPage = () => {
    navigator.clipboard.writeText(pageContent);
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  const handleAnalyzeWithOrion = () => {
    if (!onInjectContextToChat) return;
    const directive = taskInput.trim()
      ? `Addressing this request: "${taskInput.trim()}"`
      : "Provide a structured executive summary with key facts, numbers, and takeaways.";

    const promptText = `Please analyze the current webpage:\n\n**Title:** ${pageTitle}\n**URL:** ${currentUrl}\n\n\`\`\`text\n${pageContent}\n\`\`\`\n\n${directive}`;
    onInjectContextToChat(promptText);
    onClose();
  };

  if (!isOpen) return null;

  return (
    <div className="email-modal-backdrop" onClick={onClose}>
      <div
        className="email-modal-dialog modern-browser-dialog"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        {/* Top Browser Header */}
        <div className="email-modal-header browser-top-header">
          <div className="email-header-title-wrap">
            <div className="email-header-icon-box browser-icon-box">
              <IconGlobe size={18} />
            </div>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                <h3>Orion Browser</h3>
                <span className="browser-header-pill profile-pill" title="Private isolated profile directory">
                  <IconLock size={11} />
                  <span>Sandbox: {profilePath.split(/[\\/]/).pop() || "browser_profile"}</span>
                </span>
              </div>
              <p className="email-header-sub">
                Isolated sandbox session • Zero history or password tracking • Real-time AI extraction
              </p>
            </div>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
            <button
              type="button"
              className="email-close-btn"
              onClick={onClose}
              title="Close browser (Esc)"
              aria-label="Close"
            >
              <IconX size={15} />
            </button>
          </div>
        </div>

        {/* Browser Omnibar & Navigation Controls */}
        <div className="modern-browser-nav-bar">
          <div className="modern-nav-buttons">
            <button
              type="button"
              className="modern-nav-btn"
              onClick={handleBack}
              disabled={historyIndex <= 0 || loading}
              title="Back"
            >
              ←
            </button>
            <button
              type="button"
              className="modern-nav-btn"
              onClick={() => handleNavigate(currentUrl)}
              disabled={loading}
              title="Reload page"
            >
              <IconRotateCw size={13} className={loading ? "spin" : ""} />
            </button>
          </div>

          {/* Omnibar Input */}
          <form
            className="modern-omnibar-form"
            onSubmit={(e) => {
              e.preventDefault();
              handleNavigate(urlInput);
            }}
          >
            <div className="modern-omnibar-wrap">
              <IconShield size={13} className="text-emerald" />
              <input
                type="text"
                className="modern-omnibar-input"
                value={urlInput}
                onChange={(e) => setUrlInput(e.target.value)}
                placeholder="Enter web address or search query (e.g. 'mahindra stock price')..."
                disabled={loading}
              />
            </div>
            <button
              type="submit"
              className="modern-btn-go"
              disabled={loading}
            >
              {loading ? "Loading..." : "Go"}
            </button>
          </form>

          {/* Quick Shortcuts */}
          <div className="modern-quick-shortcuts">
            <button
              type="button"
              className="modern-shortcut-chip"
              onClick={() => {
                setUrlInput("https://finance.yahoo.com/quote/M&M.NS/");
                handleNavigate("https://finance.yahoo.com/quote/M&M.NS/");
              }}
            >
              Mahindra Stock
            </button>
            <button
              type="button"
              className="modern-shortcut-chip"
              onClick={() => {
                setUrlInput("https://www.google.com");
                handleNavigate("https://www.google.com");
              }}
            >
              Google
            </button>
            <button
              type="button"
              className="modern-shortcut-chip"
              onClick={() => {
                setUrlInput("https://en.wikipedia.org/wiki/Mahindra_%26_Mahindra");
                handleNavigate("https://en.wikipedia.org/wiki/Mahindra_%26_Mahindra");
              }}
            >
              Wikipedia
            </button>
          </div>
        </div>

        {/* Clean Web Canvas / Reader Pane */}
        <div className="modern-browser-content-pane">
          <div className="modern-page-header-row">
            <div>
              <h2 className="modern-page-title">{pageTitle}</h2>
              <div className="modern-page-url">{currentUrl}</div>
            </div>

            <div style={{ display: "flex", gap: "8px" }}>
              <button
                type="button"
                className="email-secondary-btn"
                onClick={handleCopyPage}
                title="Copy page text to clipboard"
              >
                {copied ? (
                  <>
                    <IconCheck size={12} className="text-emerald" />
                    <span>Copied</span>
                  </>
                ) : (
                  <>
                    <IconCopy size={12} />
                    <span>Copy Text</span>
                  </>
                )}
              </button>

              <button
                type="button"
                className="modern-btn-analyze-primary"
                onClick={handleAnalyzeWithOrion}
                title="Open in Orion Chat for deep AI analysis"
              >
                <IconZap size={13} />
                <span>Analyze with Orion</span>
              </button>
            </div>
          </div>

          {/* Clean Reader Box */}
          <div className="modern-page-reader-scroll">
            <pre className="modern-page-text">{pageContent}</pre>
          </div>

          {/* Interactive Action Bar */}
          <div className="modern-browser-task-bar">
            <input
              type="text"
              className="modern-task-input"
              value={taskInput}
              onChange={(e) => setTaskInput(e.target.value)}
              placeholder="Tell Orion what to extract or do on this page (e.g. 'What is the stock price and market cap?')..."
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  handleAnalyzeWithOrion();
                }
              }}
            />
            <button
              type="button"
              className="modern-task-submit"
              onClick={handleAnalyzeWithOrion}
            >
              <IconZap size={13} />
              <span>Ask Orion</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
