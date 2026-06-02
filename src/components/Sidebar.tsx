import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { FileEntry, SidebarView } from "../types";
import { fileIcon, fileIconColor } from "../utils/icons";
import SourceControlSidebar from "./SourceControlSidebar";
import SearchSidebar from "./SearchSidebar";
import Icon from "./Icon";

interface Props {
  view: SidebarView;
  fileTree: FileEntry[];
  activeFilePath: string;
  onOpenFile: (path: string) => void;
  onClose: (show: boolean) => void;
  workspacePath?: string;
  gitBranch?: string;
  gitRefreshTrigger?: number;
}

export default function Sidebar({ view, fileTree, activeFilePath, onOpenFile, onClose, workspacePath, gitBranch, gitRefreshTrigger }: Props) {
  switch (view) {
    case "explorer":
      return (
        <ExplorerSidebar
          fileTree={fileTree}
          activeFilePath={activeFilePath}
          onOpenFile={onOpenFile}
          onClose={onClose}
        />
      );
    case "search":
      return (
        <SearchSidebar
          workspacePath={workspacePath || ""}
          onOpenFile={onOpenFile}
          onClose={onClose}
        />
      );
    case "source-control":
      return (
        <SourceControlSidebar
          workspacePath={workspacePath || ""}
          gitBranch={gitBranch || "main"}
          onClose={onClose}
          onRefreshTrigger={gitRefreshTrigger}
        />
      );
    case "debug":
      return <PlaceholderSidebar title="RUN AND DEBUG" icon="material-symbols:play-arrow" text="Debug" />;
    case "extensions":
      return <PlaceholderSidebar title="EXTENSIONS" icon="material-symbols:extension" text="Extensions" />;
    case "ai":
      return <PlaceholderSidebar title="AI CHAT" icon="material-symbols:auto-awesome" text="AI" description="Use the right-side chat panel for AI interactions." />;
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------
// Explorer Sidebar (File Tree)
// ---------------------------------------------------------------------------

function ExplorerSidebar({
  fileTree,
  activeFilePath,
  onOpenFile,
  onClose,
}: {
  fileTree: FileEntry[];
  activeFilePath: string;
  onOpenFile: (path: string) => void;
  onClose: (show: boolean) => void;
}) {
  const [localTree, setLocalTree] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);

  // Sync local tree when fileTree prop changes
  useEffect(() => {
    if (fileTree.length > 0) {
      setLocalTree(fileTree);
    }
  }, [fileTree]);

  const toggleExpand = async (idx: number) => {
    const entry = localTree[idx];
    if (!entry.is_dir) return;

    if (entry.expanded) {
      // Collapse: remove children
      const parentDepth = entry.depth;
      let removeEnd = idx + 1;
      while (removeEnd < localTree.length && localTree[removeEnd].depth > parentDepth) {
        removeEnd++;
      }
      const updated = [...localTree];
      updated[idx] = { ...updated[idx], expanded: false };
      updated.splice(idx + 1, removeEnd - idx - 1);
      setLocalTree(updated);
    } else {
      // Expand: load children
      setLoading(true);
      try {
        const children = await invoke<FileEntry[]>("list_directory", {
          path: entry.path,
          depth: entry.depth + 1,
        });
        const updated = [...localTree];
        updated[idx] = { ...updated[idx], expanded: true };
        updated.splice(idx + 1, 0, ...children);
        setLocalTree(updated);
      } catch (e) {
        console.error("Failed to expand directory:", e);
      }
      setLoading(false);
    }
  };

  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-header-title">Explorer</span>
        <button className="sidebar-header-btn" onClick={() => onClose(false)} title="Collapse sidebar">
          −
        </button>
      </div>
      <div className="sidebar-content">
        {localTree.length === 0 && !loading && (
          <div className="sidebar-placeholder">
            <div className="sidebar-placeholder-icon">
            <Icon icon="material-symbols:folder-open" size={28} />
          </div>
            <div className="sidebar-placeholder-text">No folder open</div>
            <div className="sidebar-placeholder-text" style={{ fontSize: 11 }}>
              Open a folder to explore files
            </div>
          </div>
        )}
        {loading && (
          <div className="sidebar-placeholder">
            <div className="sidebar-placeholder-text">Loading...</div>
          </div>
        )}
        {localTree.map((entry, i) => (
          <FileTreeRow
            key={`${entry.path}-${i}`}
            entry={entry}
            isActive={entry.path === activeFilePath}
            onClick={() => {
              if (entry.is_dir) {
                toggleExpand(i);
              } else {
                onOpenFile(entry.path);
              }
            }}
          />
        ))}
      </div>
    </div>
  );
}

function FileTreeRow({
  entry,
  isActive,
  onClick,
}: {
  entry: FileEntry;
  isActive: boolean;
  onClick: () => void;
}) {
  const indent = 8 + entry.depth * 16;

  return (
    <div
      className={`file-tree-item ${isActive ? "selected" : ""}`}
      style={{ paddingLeft: indent }}
      onClick={onClick}
    >
      {/* Directory arrow */}
      {entry.is_dir ? (
        <span className="file-tree-arrow">{entry.expanded ? "▼" : "▶"}</span>
      ) : (
        <span className="file-tree-arrow" style={{ visibility: "hidden" }}>▶</span>
      )}

      {/* File icon */}
      <span className="file-tree-icon" style={{ color: fileIconColor(entry.name) }}>
        {fileIcon(entry.name)}
      </span>

      {/* File name */}
      <span className="file-tree-name">{entry.name}</span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Placeholder Sidebar
// ---------------------------------------------------------------------------

function PlaceholderSidebar({
  title,
  icon,
  text,
  description,
}: {
  title: string;
  icon: string;
  text: string;
  description?: string;
}) {
  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-header-title">{title}</span>
      </div>
      <div className="sidebar-placeholder">
        <div className="sidebar-placeholder-icon">
          <Icon icon={icon} size={28} />
        </div>
        <div className="sidebar-placeholder-text">{text}</div>
        {description && (
          <div className="sidebar-placeholder-text" style={{ fontSize: 11 }}>
            {description}
          </div>
        )}
      </div>
    </div>
  );
}


