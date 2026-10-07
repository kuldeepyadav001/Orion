import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

class ErrorBoundary extends React.Component {
  constructor(props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error) {
    return { hasError: true, error };
  }

  componentDidCatch(error, errorInfo) {
    console.error("Critical Orion React UI Error:", error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div
          style={{
            padding: "36px 32px",
            color: "#f87171",
            background: "#090d16",
            height: "100vh",
            fontFamily: "ui-monospace, monospace",
            overflow: "auto",
            boxSizing: "border-box",
          }}
        >
          <h2 style={{ color: "#ef4444", margin: "0 0 14px", fontSize: "18px" }}>
            ⚠️ Orion Interface Diagnostic
          </h2>
          <p style={{ color: "#94a3b8", fontSize: "13px", margin: "0 0 16px" }}>
            An unexpected error occurred during rendering. You can reload the window below:
          </p>
          <pre
            style={{
              background: "rgba(255, 255, 255, 0.05)",
              border: "1px solid rgba(255, 255, 255, 0.1)",
              padding: "16px",
              borderRadius: "8px",
              whiteSpace: "pre-wrap",
              fontSize: "12px",
              color: "#fca5a5",
            }}
          >
            {String(this.state.error?.stack || this.state.error?.message || this.state.error)}
          </pre>
          <button
            type="button"
            onClick={() => window.location.reload()}
            style={{
              marginTop: "20px",
              padding: "8px 18px",
              background: "#2563eb",
              color: "#fff",
              border: "none",
              borderRadius: "6px",
              cursor: "pointer",
              fontWeight: "600",
              fontSize: "13px",
            }}
          >
            Reload Interface
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
