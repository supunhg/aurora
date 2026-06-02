import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { GitFileStatus, GitCommitInfo, GitBranchInfo, GitStashEntry } from "../types";
import { fileIcon, fileIconColor } from "../utils/icons";
import Icon from "./Icon";

interface Props {
  workspacePath: string;
  gitBranch: string;
  onClose: (show: boolean) => void;
  onRefreshTrigger?: number;
}

export default function SourceControlSidebar({
  workspacePath,
  gitBranch,
  onClose,
  onRefreshTrigger = 0,
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

  // Branch switcher state
  const [showBranchSwitcher, setShowBranchSwitcher] = useState(false);
  const [branches, setBranches] = useState<GitBranchInfo[]>([]);
  const [branchesLoading, setBranchesLoading] = useState(false);
  const [branchSearch, setBranchSearch] = useState("");
  const [switchingBranch, setSwitchingBranch] = useState(false);

  // Stash state
  const [showStashPanel, setShowStashPanel] = useState(false);
  const [stashes, setStashes] = useState<GitStashEntry[]>([]);
  const [stashesLoading, setStashesLoading] = useState(false);
  const [stashMessage, setStashMessage] = useState("");
  const [stashing, setStashing] = useState(false);

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

  // Re-fetch when refresh trigger changes (e.g. file edits in Monaco)
  useEffect(() => {
    fetchStatus();
  }, [fetchStatus, onRefreshTrigger]);

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
    const confirmed = window.confirm(
      `Discard all changes to "${filePath}"?\n\nThis action cannot be undone.`
    );
    if (!confirmed) return;
    try {
      await invoke("git_discard_file", { path: gitRoot, filePath });
      await fetchStatus();
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
      setCommitResult(`\u2713 Committed: ${result.message.substring(0, 50)}`);
      setCommitMessage("");
      await fetchStatus();
      setSelectedFile(null);
      setDiffContent(null);
    } catch (e) {
      setCommitResult(`\u2717 Error: ${e}`);
    }
    setCommitting(false);
  };

  // --- Branch Switcher ---

  const openBranchSwitcher = async () => {
    if (!gitRoot) return;
    setShowBranchSwitcher(true);
    setBranchSearch("");
    setBranchesLoading(true);
    try {
      const list = await invoke<GitBranchInfo[]>("git_list_branches", { path: gitRoot });
      setBranches(list);
    } catch (e) {
      console.error("Failed to list branches:", e);
    }
    setBranchesLoading(false);
  };

  const handleSwitchBranch = async (branchName: string) => {
    if (!gitRoot) return;
    setSwitchingBranch(true);
    try {
      await invoke("git_switch_branch", { path: gitRoot, branchName, createNew: false });
      setShowBranchSwitcher(false);
      await fetchStatus();
      window.location.reload();
    } catch (e) {
      alert(`Failed to switch branch: ${e}`);
    }
    setSwitchingBranch(false);
  };

  const handleCreateBranch = async () => {
    if (!branchSearch.trim() || !gitRoot) return;
    setSwitchingBranch(true);
    try {
      await invoke("git_create_branch", {
        path: gitRoot,
        branchName: branchSearch.trim(),
        baseBranch: null,
      });
      setShowBranchSwitcher(false);
      await fetchStatus();
      window.location.reload();
    } catch (e) {
      alert(`Failed to create branch: ${e}`);
    }
    setSwitchingBranch(false);
  };

  const filteredBranches = branchSearch
    ? branches.filter((b) => b.name.toLowerCase().includes(branchSearch.toLowerCase()))
    : branches;

  // --- Stash Operations ---

  const openStashPanel = async () => {
    if (!gitRoot) return;
    setShowStashPanel(true);
    setStashMessage("");
    await refreshStashList();
  };

  const refreshStashList = async () => {
    if (!gitRoot) return;
    setStashesLoading(true);
    try {
      const list = await invoke<GitStashEntry[]>("git_stash_list", { path: gitRoot });
      setStashes(list);
    } catch (e) {
      console.error("Failed to list stashes:", e);
    }
    setStashesLoading(false);
  };

  const handleStashPush = async () => {
    if (!gitRoot) return;
    setStashing(true);
    try {
      await invoke("git_stash_push", {
        path: gitRoot,
        message: stashMessage.trim() || null,
      });
      setStashMessage("");
      await fetchStatus();
      await refreshStashList();
    } catch (e) {
      alert(`Failed to stash: ${e}`);
    }
    setStashing(false);
  };

  const handleStashPop = async (index?: number) => {
    if (!gitRoot) return;
    const confirmed = index !== undefined
      ? window.confirm(`Pop and apply stash@{${index}}?`)
      : window.confirm("Pop and apply the latest stash?");
    if (!confirmed) return;
    try {
      await invoke("git_stash_pop", { path: gitRoot, index: index ?? null });
      await refreshStashList();
      await fetchStatus();
    } catch (e) {
      alert(`Failed to pop stash: ${e}`);
    }
  };

  const handleStashDrop = async (index: number) => {
    if (!gitRoot) return;
    const confirmed = window.confirm(`Delete stash@{${index}}?`);
    if (!confirmed) return;
    try {
      await invoke("git_stash_drop", { path: gitRoot, index });
      await refreshStashList();
    } catch (e) {
      alert(`Failed to drop stash: ${e}`);
    }
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
          <Icon icon="material-symbols:refresh" size={14} />
        </button>
        <button className="sidebar-header-btn" onClick={() => onClose(false)} title="Collapse sidebar">
          <Icon icon="material-symbols:close" size={14} />
        </button>
      </div>

      <div className="sc-branch-bar">
        <Icon icon="material-symbols:call-split" size={13} style={{ color: "var(--text-muted)" }} />
        <span
          className="sc-branch-name sc-branch-clickable"
          onClick={openBranchSwitcher}
          title="Switch branch"
        >
          {gitBranch}
        </span>
        <span className="sc-branch-actions">
          <button
            className="sc-header-action-btn"
            onClick={openBranchSwitcher}
            title="Switch Branch"
          >
            <Icon icon="material-symbols:call-split" size={12} />
          </button>
          <button
            className="sc-header-action-btn"
            onClick={openStashPanel}
            title="Stash"
          >
            <Icon icon="material-symbols:content-save" size={12} />
          </button>
        </span>
      </div>

      {/* Commit area */}
      {!showBranchSwitcher && !showStashPanel && (
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
                <Icon icon="material-symbols:add" size={12} style={{ marginRight: 2 }} />
                All
              </button>
            )}
          </div>
          {commitResult && (
            <div className={`sc-commit-result ${commitResult.startsWith("\u2713") ? "success" : "error"}`}>
              {commitResult}
            </div>
          )}
        </div>
      )}

      {/* Branch Switcher Panel */}
      {showBranchSwitcher && (
        <div className="sc-branch-panel">
          <div className="sc-branch-panel-header">
            <span className="sc-branch-panel-title">Switch Branch</span>
            <button className="sc-diff-close" onClick={() => setShowBranchSwitcher(false)}>
              <Icon icon="material-symbols:close" size={14} />
            </button>
          </div>
          <input
            className="sc-branch-search"
            type="text"
            placeholder="Search or create branch..."
            value={branchSearch}
            onChange={(e) => setBranchSearch(e.target.value)}
            autoFocus
            onKeyDown={(e) => {
              if (e.key === "Enter" && branchSearch.trim()) {
                const exact = branches.find((b) => b.name === branchSearch.trim());
                if (exact) {
                  handleSwitchBranch(exact.name);
                } else {
                  handleCreateBranch();
                }
              }
              if (e.key === "Escape") {
                setShowBranchSwitcher(false);
              }
            }}
          />
          <div className="sc-branch-list">
            {branchesLoading && <div className="sc-empty-text">Loading branches...</div>}
            {filteredBranches.map((branch) => (
              <div
                key={branch.name}
                className={`sc-branch-item ${branch.current ? "current" : ""}`}
                onClick={() => !branch.current && handleSwitchBranch(branch.name)}
              >
                <span className="sc-branch-item-icon">
                  {branch.current ? (
                    <Icon icon="material-symbols:check" size={12} />
                  ) : (
                    <Icon icon="material-symbols:call-split" size={12} />
                  )}
                </span>
                <span className="sc-branch-item-name">{branch.name}</span>
                {branch.upstream && (
                  <span className="sc-branch-item-upstream">{branch.upstream}</span>
                )}
              </div>
            ))}
            {branchSearch.trim() && !filteredBranches.find((b) => b.name === branchSearch.trim()) && (
              <div className="sc-branch-item create" onClick={handleCreateBranch}>
                <span className="sc-branch-item-icon">
                  <Icon icon="material-symbols:add" size={12} />
                </span>
                <span className="sc-branch-item-name">Create branch &ldquo;{branchSearch.trim()}&rdquo;</span>
              </div>
            )}
            {switchingBranch && <div className="sc-empty-text">Switching branch...</div>}
          </div>
        </div>
      )}

      {/* Stash Panel */}
      {showStashPanel && (
        <div className="sc-branch-panel">
          <div className="sc-branch-panel-header">
            <span className="sc-branch-panel-title">Stashes</span>
            <button className="sc-diff-close" onClick={() => { setShowStashPanel(false); refreshStashList(); }}>
              <Icon icon="material-symbols:close" size={14} />
            </button>
          </div>
          <div className="sc-stash-push-area">
            <input
              className="sc-branch-search"
              type="text"
              placeholder="Stash message (optional)"
              value={stashMessage}
              onChange={(e) => setStashMessage(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleStashPush();
              }}
            />
            <button
              className="sc-btn sc-btn-commit"
              onClick={handleStashPush}
              disabled={stashing}
              style={{ marginTop: 4 }}
            >
              {stashing ? "Stashing..." : "Stash Changes"}
            </button>
          </div>
          <div className="sc-branch-list">
            {stashesLoading && <div className="sc-empty-text">Loading stashes...</div>}
            {!stashesLoading && stashes.length === 0 && (
              <div className="sc-empty-text" style={{ padding: 16, textAlign: "center" }}>No stashes</div>
            )}
            {stashes.map((stash) => (
              <div key={stash.index} className="sc-branch-item">
                <span className="sc-branch-item-icon">
                  <Icon icon="material-symbols:content-save" size={12} />
                </span>
                <span className="sc-branch-item-name" style={{ flex: 1 }}>
                  stash@{stash.index}: {stash.message}
                </span>
                <div className="sc-file-actions">
                  <button className="sc-action-btn" onClick={() => handleStashPop(stash.index)} title="Apply and drop">
                    <Icon icon="material-symbols:unfold-more" size={12} />
                  </button>
                  <button className="sc-action-btn sc-action-discard" onClick={() => handleStashDrop(stash.index)} title="Drop stash">
                    <Icon icon="material-symbols:delete-outline" size={12} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Main content */}
      {!showBranchSwitcher && !showStashPanel && (
        <div className="sidebar-content sc-content">
          {error && (
            <div className="sc-empty">
              <div className="sc-empty-icon">
                <Icon icon="material-symbols:call-split" size={28} />
              </div>
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
              <div className="sc-empty-icon">
                <Icon icon="material-symbols:check-circle" size={28} />
              </div>
              <div className="sc-empty-text">No changes</div>
              <div className="sc-empty-detail">Working tree is clean</div>
            </div>
          )}

          {!error && hasChanges && (
            <>
              {stagedChanges.length > 0 && (
                <div className="sc-section">
                  <div className="sc-section-header" onClick={() => setShowStaged(!showStaged)}>
                    <span className="sc-section-arrow">{showStaged ? "\u25BC" : "\u25B6"}</span>
                    <span className="sc-section-title">Staged Changes</span>
                    <span className="sc-section-count">{stagedChanges.length}</span>
                  </div>
                  {showStaged && (
                    <div className="sc-file-list">
                      {stagedChanges.map((file) => {
                        const badge = statusBadge(file.status);
                        return (
                          <div key={`staged-${file.path}`} className={`sc-file-item ${selectedFile === file.path ? "selected" : ""}`}>
                            <div className="sc-file-main" onClick={() => handleShowDiff(file.path, true)}>
                              <span className={`sc-badge ${badge.cls}`}>{badge.label}</span>
                              <span className="sc-file-icon" style={{ color: fileIconColor(file.path) }}>{fileIcon(file.path)}</span>
                              <span className="sc-file-name">{file.path}</span>
                            </div>
                            <div className="sc-file-actions">
                              <button className="sc-action-btn" onClick={() => handleUnstageFile(file.path)} title="Unstage">
                                <Icon icon="material-symbols:remove" size={12} />
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              )}

              {unstagedChanges.length > 0 && (
                <div className="sc-section">
                  <div className="sc-section-header" onClick={() => setShowUnstaged(!showUnstaged)}>
                    <span className="sc-section-arrow">{showUnstaged ? "\u25BC" : "\u25B6"}</span>
                    <span className="sc-section-title">Changes</span>
                    <span className="sc-section-count">{unstagedChanges.length}</span>
                  </div>
                  {showUnstaged && (
                    <div className="sc-file-list">
                      {unstagedChanges.map((file) => {
                        const badge = statusBadge(file.status);
                        return (
                          <div key={`unstaged-${file.path}`} className={`sc-file-item ${selectedFile === file.path ? "selected" : ""}`}>
                            <div className="sc-file-main" onClick={() => handleShowDiff(file.path, false)}>
                              <span className={`sc-badge ${badge.cls}`}>{badge.label}</span>
                              <span className="sc-file-icon" style={{ color: fileIconColor(file.path) }}>{fileIcon(file.path)}</span>
                              <span className="sc-file-name">{file.path}</span>
                            </div>
                            <div className="sc-file-actions">
                              <button className="sc-action-btn" onClick={() => handleStageFile(file.path)} title="Stage">
                                <Icon icon="material-symbols:add" size={12} />
                              </button>
                              <button className="sc-action-btn sc-action-discard" onClick={() => handleDiscardFile(file.path)} title="Discard Changes">
                                <Icon icon="material-symbols:undo" size={12} />
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              )}

              {untrackedFiles.length > 0 && (
                <div className="sc-section">
                  <div className="sc-section-header" onClick={() => setShowUntracked(!showUntracked)}>
                    <span className="sc-section-arrow">{showUntracked ? "\u25BC" : "\u25B6"}</span>
                    <span className="sc-section-title">Untracked</span>
                    <span className="sc-section-count">{untrackedFiles.length}</span>
                  </div>
                  {showUntracked && (
                    <div className="sc-file-list">
                      {untrackedFiles.map((file) => (
                        <div key={`untracked-${file.path}`} className={`sc-file-item ${selectedFile === file.path ? "selected" : ""}`}>
                          <div className="sc-file-main" onClick={() => handleShowDiff(file.path, false)}>
                            <span className="sc-badge badge-untracked">U</span>
                            <span className="sc-file-icon" style={{ color: fileIconColor(file.path) }}>{fileIcon(file.path)}</span>
                            <span className="sc-file-name">{file.path}</span>
                          </div>
                          <div className="sc-file-actions">
                            <button className="sc-action-btn" onClick={() => handleStageFile(file.path)} title="Stage">
                              <Icon icon="material-symbols:add" size={12} />
                            </button>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {selectedFile && (
                <div className="sc-diff-section">
                  <div className="sc-diff-header">
                    <span className="sc-diff-title">Diff: {selectedFile}</span>
                    <button className="sc-diff-close" onClick={() => { setSelectedFile(null); setDiffContent(null); }}>
                      <Icon icon="material-symbols:close" size={14} />
                    </button>
                  </div>
                  <div className="sc-diff-content">
                    {diffLoading ? (
                      <div className="sc-diff-loading">Loading diff...</div>
                    ) : diffContent ? (
                      <pre className="sc-diff-pre"><code>{renderDiffLines(diffContent)}</code></pre>
                    ) : null}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
