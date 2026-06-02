import type { OpenFile } from "../types";
import Icon from "./Icon";

interface Props {
  activeFile: OpenFile | null;
  gitBranch: string;
  isAiThinking: boolean;
  aiStatus: string;
  watcherActive?: boolean;
  workspacePath?: string;
}

export default function StatusBar({
  activeFile,
  gitBranch,
  isAiThinking,
  aiStatus,
  watcherActive = true,
  workspacePath = "",
}: Props) {
  return (
    <div className="status-bar">
      {/* Left section */}
      <div className="status-bar-left">
        <span className="status-bar-item" style={{ color: "var(--text-secondary)" }}>
          <Icon icon="material-symbols:call-split" size={12} style={{ marginRight: 2 }} />
          {gitBranch}
        </span>
        {activeFile?.modified && (
          <span className="status-bar-item" style={{ color: "var(--status-modified)" }}>
            <Icon icon="material-symbols:circle" size={10} style={{ marginRight: 2 }} />
            Modified
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
        {/* File watcher status */}
        <span
          className="status-bar-item"
          title={watcherActive ? `Watching: ${workspacePath}` : "File watcher inactive"}
          style={{ color: watcherActive ? "var(--accent-green)" : "var(--text-muted)" }}
        >
          <Icon
            icon={watcherActive ? "material-symbols:visibility" : "material-symbols:visibility-off"}
            size={12}
            style={{ marginRight: 2 }}
          />
          FS
        </span>

        <span
          className="status-bar-item"
          style={{ color: isAiThinking ? "var(--status-ai-thinking)" : "var(--status-ai-ready)" }}
        >
          {isAiThinking ? (
            <Icon icon="material-symbols:progress-activity" size={12} style={{ marginRight: 2 }} />
          ) : (
            <Icon icon="material-symbols:check-circle" size={12} style={{ marginRight: 2 }} />
          )}
          {isAiThinking ? "Thinking..." : aiStatus}
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
