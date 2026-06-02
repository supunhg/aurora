import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { GitFileStatus, GitCommitInfo } from "../types";
import { fileIcon, fileIconColor } from "../utils/icons";

interface Props {
  workspacePath: string;
  gitBranch: string;
  onClose: (show: boolean) => void;
}

export default function SourceControlSidebar({
  workspacePath,
  gitBranch,
  onClose,
}: Props) {
  const [gitStatus, setGitStatus] = useState<GitFileStatus[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [diffContent, setDiffContent] = useState<string | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);
  const [commitMessage, setCommitMessage] = useState("");
  const [committing, setCommitting] = useState(false);
  const [commitResult, setCommitResult] = useState<string | null>(null);
  const [showStaged, setShowStaged] = useState(true);
  const [showUnstaged, setShowUnstaged] = useState(true);
  const [showUntracked, setShowUntracked] = useState(true);
  const [gitRoot, setGitRoot] = useState("");
  const [refreshing, setRefreshing] = useState(false);

  const fetchStatus = useCallback(async () => {
    if (!workspacePath) return;
    setLoading(true);
    setError(null);
    try {
      const root = await invoke<string>("get_git_root", { path: workspacePath });
      setGitRoot(root);
      const status = await invoke<GitFileStatus[]>("git_status", { path: root });
      setGitStatus(status);
    } catch (e) {
      setError(String(e));
      setGitStatus([]);
    }
    setLoading(false);
  }, [workspacePath]);

  useEffect(() => {
    fetchStatus();
  }, [fetchStatus]);

  const handleRefresh = async () => {
    setRefreshing(true);
    await fetchStatus();
    setRefreshing(false);
  };

  const handleStageFile = async (filePath: string) => {
    if (!gitRoot) return;
    try {
      await invoke("git_stage_file", { path: gitRoot, filePath });
      await fetchStatus();
    } catch (e) {
      console.error("Failed to stage file:", e);
    }
  };

  const handleUnstageFile = async (filePath: string) => {
    if (!gitRoot) return;
    try {
      await invoke("git_unstage_file", { path: gitRoot, filePath });
      await fetchStatus();
    } catch (e) {
      console.error("Failed to unstage file:", e);
    }
  };

  const handleDiscardFile = async (filePath: string) => {
    if (!gitRoot) return;
    try {
      await invoke("git_discard_file", { path: gitRoot, filePath });
      await fetchStatus();
      // Deselect if selected
      if (selectedFile === filePath) {
        setSelectedFile(null);
        setDiffContent(null);
      }
    } catch (e) {
      console.error("Failed to discard file:", e);
    }
  };

  const handleStageAll = async () => {
    if (!gitRoot) return;
    try {
      await invoke("git_stage_all", { path: gitRoot });
      await fetchStatus();
    } catch (e) {
      console.error("Failed to stage all:", e);
    }
  };

  const handleShowDiff = async (filePath: string, staged: boolean) => {
    if (!gitRoot) return;
    setSelectedFile(filePath);
    setDiffLoading(true);
    setDiffContent(null);
    try {
      const diff = await invoke<string>("git_show_diff", {
        path: gitRoot,
        filePath,
        staged,
      });
      setDiffContent(diff || "(binary or no changes)");
    } catch (e) {
      setDiffContent(`Error loading diff: ${e}`);
    }
    setDiffLoading(false);
  };

  const handleCommit = async () => {
    if (!commitMessage.trim() || !gitRoot) return;
    setCommitting(true);
    setCommitResult(null);
    try {
      const result = await invoke<GitCommitInfo>("git_commit", {
        path: gitRoot,
        message: commitMessage.trim(),
      });
      setCommitResult(`✓ Committed: ${result.message.substring(0, 50)}`);
      setCommitMessage("");
      await fetchStatus();
      setSelectedFile(null);
      setDiffContent(null);
    } catch (e) {
      setCommitResult(`✗ Error: ${e}`);
    }
    setCommitting(false);
  };

  const stagedChanges = gitStatus.filter((f) => f.staged);
  const unstagedChanges = gitStatus.filter((f) => !f.staged && f.status !== "??");
  const untrackedFiles = gitStatus.filter((f) => f.status === "??");

  const statusBadge = (status: string) => {
    const first = status.charAt(0) || "";
    if (status === "??") return { label: "U", cls: "badge-untracked" };
    if (status.includes("M")) return { label: "M", cls: "badge-modified" };
    if (status.includes("A")) return { label: "A", cls: "badge-added" };
    if (status.includes("D")) return { label: "D", cls: "badge-deleted" };
    if (status.includes("R")) return { label: "R", cls: "badge-renamed" };
    return { label: first, cls: "badge-default" };
  };

  // Parse diff for syntax highlighting
  const renderDiffLines = (diff: string) => {
    return diff.split("\n").map((line, i) => {
      let cls = "diff-line";
      let prefix = " ";
      if (line.startsWith("+++") || line.startsWith("---")) {
        cls += " diff-header";
      } else if (line.startsWith("@@")) {
        cls += " diff-hunk";
        prefix = "";
      } else if (line.startsWith("+")) {
        cls += " diff-added";
        prefix = "+";
      } else if (line.startsWith("-")) {
        cls += " diff-removed";
        prefix = "-";
      }
      return (
        <div key={i} className={cls}>
          <span className="diff-prefix">{prefix}</span>
          <span className="diff-text">{line}</span>
        </div>
      );
    });
  };

  const hasChanges = stagedChanges.length > 0 || unstagedChanges.length > 0 || untrackedFiles.length > 0;

  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-header-title">Source Control</span>
        <button
          className={`sidebar-header-btn ${refreshing ? "spinning" : ""}`}
          onClick={handleRefresh}
          title="Refresh"
        >
          ⟳
        </button>
        <button className="sidebar-header-btn" onClick={() => onClose(false)} title="Collapse sidebar">
          −
        </button>
      </div>

      <div className="sc-branch-bar">
        <span className="sc-branch-icon">⎇</span>
        <span className="sc-branch-name">{gitBranch}</span>
      </div>

      {/* Commit area */}
      <div className="sc-commit-area">
        <textarea
          className="sc-commit-input"
          placeholder="Message (Ctrl+Enter to commit)"
          value={commitMessage}
          onChange={(e) => setCommitMessage(e.target.value)}
          onKeyDown={(e) => {
            if (e.ctrlKey && e.key === "Enter") {
              handleCommit();
            }
          }}
          rows={3}
          disabled={committing}
        />
        <div className="sc-commit-actions">
          <button
            className="sc-btn sc-btn-commit"
            disabled={!commitMessage.trim() || committing || stagedChanges.length === 0}
            onClick={handleCommit}
          >
            {committing ? "Committing..." : "Commit"}
          </button>
          {unstagedChanges.length + untrackedFiles.length > 0 && (
            <button className="sc-btn sc-btn-stage-all" onClick={handleStageAll} title="Stage All Changes">
              + All
            </button>
          )}
        </div>
        {commitResult && (
          <div className={`sc-commit-result ${commitResult.startsWith("✓") ? "success" : "error"}`}>
            {commitResult}
          </div>
        )}
      </div>

      <div className="sidebar-content sc-content">
        {error && (
          <div className="sc-empty">
            <div className="sc-empty-icon">⎇</div>
            <div className="sc-empty-text">No git repository</div>
            <div className="sc-empty-detail">Open a folder with a git repo to see changes</div>
          </div>
        )}

        {!error && loading && (
          <div className="sc-empty">
            <div className="sc-empty-text">Loading status...</div>
          </div>
        )}

        {!error && !loading && !hasChanges && (
          <div className="sc-empty">
            <div className="sc-empty-icon">✓</div>
            <div className="sc-empty-text">No changes</div>
            <div className="sc-empty-detail">Working tree is clean</div>
          </div>
        )}

        {!error && hasChanges && (
          <>
            {/* Staged Changes */}
            {stagedChanges.length > 0 && (
              <div className="sc-section">
                <div
                  className="sc-section-header"
                  onClick={() => setShowStaged(!showStaged)}
                >
                  <span className="sc-section-arrow">{showStaged ? "▼" : "▶"}</span>
                  <span className="sc-section-title">Staged Changes</span>
                  <span className="sc-section-count">{stagedChanges.length}</span>
                </div>
                {showStaged && (
                  <div className="sc-file-list">
                    {stagedChanges.map((file) => {
                      const badge = statusBadge(file.status);
                      return (
                        <div
                          key={`staged-${file.path}`}
                          className={`sc-file-item ${selectedFile === file.path ? "selected" : ""}`}
                        >
                          <div
                            className="sc-file-main"
                            onClick={() => handleShowDiff(file.path, true)}
                          >
                            <span className={`sc-badge ${badge.cls}`}>{badge.label}</span>
                            <span className="sc-file-icon" style={{ color: fileIconColor(file.path) }}>
                              {fileIcon(file.path)}
                            </span>
                            <span className="sc-file-name">{file.path}</span>
                          </div>
                          <div className="sc-file-actions">
                            <button
                              className="sc-action-btn"
                              onClick={() => handleUnstageFile(file.path)}
                              title="Unstage"
                            >
                              −
                            </button>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}

            {/* Unstaged Changes */}
            {unstagedChanges.length > 0 && (
              <div className="sc-section">
                <div
                  className="sc-section-header"
                  onClick={() => setShowUnstaged(!showUnstaged)}
                >
                  <span className="sc-section-arrow">{showUnstaged ? "▼" : "▶"}</span>
                  <span className="sc-section-title">Changes</span>
                  <span className="sc-section-count">{unstagedChanges.length}</span>
                </div>
                {showUnstaged && (
                  <div className="sc-file-list">
                    {unstagedChanges.map((file) => {
                      const badge = statusBadge(file.status);
                      return (
                        <div
                          key={`unstaged-${file.path}`}
                          className={`sc-file-item ${selectedFile === file.path ? "selected" : ""}`}
                        >
                          <div
                            className="sc-file-main"
                            onClick={() => handleShowDiff(file.path, false)}
                          >
                            <span className={`sc-badge ${badge.cls}`}>{badge.label}</span>
                            <span className="sc-file-icon" style={{ color: fileIconColor(file.path) }}>
                              {fileIcon(file.path)}
                            </span>
                            <span className="sc-file-name">{file.path}</span>
                          </div>
                          <div className="sc-file-actions">
                            <button
                              className="sc-action-btn"
                              onClick={() => handleStageFile(file.path)}
                              title="Stage"
                            >
                              +
                            </button>
                            <button
                              className="sc-action-btn sc-action-discard"
                              onClick={() => handleDiscardFile(file.path)}
                              title="Discard Changes"
                            >
                              ↶
                            </button>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}

            {/* Untracked Files */}
            {untrackedFiles.length > 0 && (
              <div className="sc-section">
                <div
                  className="sc-section-header"
                  onClick={() => setShowUntracked(!showUntracked)}
                >
                  <span className="sc-section-arrow">{showUntracked ? "▼" : "▶"}</span>
                  <span className="sc-section-title">Untracked</span>
                  <span className="sc-section-count">{untrackedFiles.length}</span>
                </div>
                {showUntracked && (
                  <div className="sc-file-list">
                    {untrackedFiles.map((file) => (
                      <div
                        key={`untracked-${file.path}`}
                        className={`sc-file-item ${selectedFile === file.path ? "selected" : ""}`}
                      >
                        <div
                          className="sc-file-main"
                          onClick={() => handleShowDiff(file.path, false)}
                        >
                          <span className="sc-badge badge-untracked">U</span>
                          <span className="sc-file-icon" style={{ color: fileIconColor(file.path) }}>
                            {fileIcon(file.path)}
                          </span>
                          <span className="sc-file-name">{file.path}</span>
                        </div>
                        <div className="sc-file-actions">
                          <button
                            className="sc-action-btn"
                            onClick={() => handleStageFile(file.path)}
                            title="Stage"
                          >
                            +
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}

            {/* Diff Viewer */}
            {selectedFile && (
              <div className="sc-diff-section">
                <div className="sc-diff-header">
                  <span className="sc-diff-title">Diff: {selectedFile}</span>
                  <button
                    className="sc-diff-close"
                    onClick={() => {
                      setSelectedFile(null);
                      setDiffContent(null);
                    }}
                  >
                    ×
                  </button>
                </div>
                <div className="sc-diff-content">
                  {diffLoading ? (
                    <div className="sc-diff-loading">Loading diff...</div>
                  ) : diffContent ? (
                    <pre className="sc-diff-pre">
                      <code>{renderDiffLines(diffContent)}</code>
                    </pre>
                  ) : null}
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
