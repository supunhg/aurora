import type { OpenFile } from "../types";

interface Props {
  activeFile: OpenFile | null;
  gitBranch: string;
  isAiThinking: boolean;
  aiStatus: string;
}

export default function StatusBar({
  activeFile,
  gitBranch,
  isAiThinking,
  aiStatus,
}: Props) {
  return (
    <div className="status-bar">
      {/* Left section */}
      <div className="status-bar-left">
        <span className="status-bar-item" style={{ color: "var(--text-secondary)" }}>
          ⎇ {gitBranch}
        </span>
        {activeFile?.modified && (
          <span className="status-bar-item" style={{ color: "var(--status-modified)" }}>
            ● Modified
          </span>
        )}
        {activeFile && (
          <>
            <span className="status-bar-item" style={{ color: "var(--text-primary)" }}>
              {activeFile.name}
            </span>
            <span className="status-bar-item" style={{ color: "var(--text-muted)" }}>
              {activeFile.language}
            </span>
          </>
        )}
      </div>

      {/* Right section */}
      <div className="status-bar-right">
        <span
          className="status-bar-item"
          style={{ color: isAiThinking ? "var(--status-ai-thinking)" : "var(--status-ai-ready)" }}
        >
          {isAiThinking ? "⟳ Thinking..." : aiStatus}
        </span>
        <span className="status-bar-item" style={{ color: "var(--text-muted)" }}>
          UTF-8
        </span>
        <span className="status-bar-item" style={{ color: "var(--text-muted)" }}>
          Spaces: 4
        </span>
        <span className="status-bar-item" style={{ color: "var(--text-primary)" }}>
          Ln 1, Col 1
        </span>
      </div>
    </div>
  );
}
