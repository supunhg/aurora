import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { FileEntry, ChatMessage, SidebarView, OpenFile, FileWatchEvent } from "./types";
import { fileIcon } from "./utils/icons";
import ActivityBar from "./components/ActivityBar";
import Sidebar from "./components/Sidebar";
import Editor from "./components/Editor";
import StatusBar from "./components/StatusBar";
import ChatPanel from "./components/ChatPanel";
import "./App.css";

// Extend Window to track watcher state (simple approach)
declare global {
  interface Window {
    __watcherActive?: boolean;
  }
}

export default function App() {
  const [activeSidebar, setActiveSidebar] = useState<SidebarView>("explorer");
  const [showSidebar, setShowSidebar] = useState(true);
  const [showChat, setShowChat] = useState(true);
  const [openFiles, setOpenFiles] = useState<OpenFile[]>([]);
  const [activeFile, setActiveFile] = useState<OpenFile | null>(null);
  const [fileTree, setFileTree] = useState<FileEntry[]>([]);
  const [chatMessages, setChatMessages] = useState<ChatMessage[]>([]);
  const [isAiThinking, setIsAiThinking] = useState(false);
  const [gitBranch, setGitBranch] = useState("main");
  const [workspacePath, setWorkspacePath] = useState("");
  const [gitRoot, setGitRoot] = useState("");
  const [watcherActive, setWatcherActive] = useState(false);
  // Debounced trigger for git status auto-refresh on file edits
  const [gitRefreshTrigger, setGitRefreshTrigger] = useState(0);
  const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Cleanup debounce timer on unmount
  useEffect(() => {
    return () => {
      if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current);
    };
  }, []);

  // Listen for file watcher events from Tauri backend
  useEffect(() => {
    const unlisten = listen<FileWatchEvent>("file-changed", (event) => {
      const { kind } = event.payload;
      if (kind === "modified" || kind === "created" || kind === "deleted") {
        // Re-list the directory and trigger git status refresh
        // (debounced to avoid rapid re-fetches)
        if (refreshTimerRef.current) {
          clearTimeout(refreshTimerRef.current);
        }
        refreshTimerRef.current = setTimeout(async () => {
          try {
            const entries = await invoke<FileEntry[]>("list_directory", {
              path: workspacePath,
              depth: 0,
            });
            setFileTree(entries);
            setGitRefreshTrigger((prev) => prev + 1);
          } catch {
            // Ignore errors during watcher refresh
          }
        }, 500);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [workspacePath]);

  // Load initial file tree from current directory
  useEffect(() => {
    const init = async () => {
      try {
        const cwd = await invoke<string>("get_current_dir");
        setWorkspacePath(cwd);
        // Get git root
        try {
          const root = await invoke<string>("get_git_root", { path: cwd });
          setGitRoot(root);
        } catch {
          // Not a git repo
        }
        // Start file watcher
        try {
          await invoke("start_file_watcher", { path: cwd });
          setWatcherActive(true);
          window.__watcherActive = true;
        } catch (e) {
          console.error("Failed to start file watcher:", e);
        }
        const entries = await invoke<FileEntry[]>("list_directory", {
          path: cwd,
          depth: 0,
        });
        setFileTree(entries);
        const branch = await invoke<string>("get_git_branch", { path: cwd });
        setGitBranch(branch);
      } catch {
        // Running outside Tauri (e.g., in browser for dev)
      }
    };
    init();
  }, []);

  const handleOpenFile = useCallback(async (path: string) => {
    try {
      const content = await invoke<string>("read_file", { path });
      const name = path.split("/").pop() || path.split("\\").pop() || "untitled";
      const language = await invoke<string>("detect_language", { path });

      // Check if already open
      const existing = openFiles.findIndex((f) => f.path === path);
      if (existing >= 0) {
        setActiveFile(openFiles[existing]);
        return;
      }

      const file: OpenFile = { name, path, content, modified: false, language };
      const updated = [...openFiles, file];
      setOpenFiles(updated);
      setActiveFile(file);
    } catch (e) {
      console.error("Failed to open file:", e);
    }
  }, [openFiles]);

  const handleCloseFile = useCallback((path: string) => {
    setOpenFiles((prev) => {
      const idx = prev.findIndex((f) => f.path === path);
      if (idx < 0) return prev;
      const updated = prev.filter((_, i) => i !== idx);
      // If closing active file, switch to another
      if (activeFile?.path === path) {
        const nextIdx = Math.min(idx, updated.length - 1);
        setActiveFile(updated[nextIdx] || null);
      }
      return updated;
    });
  }, [activeFile]);

  const handleSwitchFile = useCallback((file: OpenFile) => {
    setActiveFile(file);
  }, []);

  // Handle Ctrl+S save from Monaco editor
  const handleSave = useCallback(async (path: string, content: string) => {
    try {
      await invoke("write_file", { path, content });
      setOpenFiles((prev) =>
        prev.map((f) => (f.path === path ? { ...f, content, modified: false } : f))
      );
    } catch (e) {
      console.error("Failed to save file:", e);
    }
  }, []);

  const handleSendChat = useCallback(async (message: string) => {
    if (!message.trim() || isAiThinking) return;

    const userMsg: ChatMessage = { role: "user", content: message, streaming: false };
    setChatMessages((prev) => [...prev, userMsg]);
    setIsAiThinking(true);

    try {
      const context = activeFile
        ? `Currently editing: ${activeFile.name}\n\nFile contents:\n${activeFile.content}\n\nUser request: ${message}`
        : message;

      const response = await invoke<string>("chat_completion", {
        model: "auto",
        messages: [
          {
            role: "system",
            content: "You are Aurora, an AI coding assistant. Help with software engineering tasks. Be concise and helpful.",
          },
          { role: "user", content: context },
        ],
      });

      const assistantMsg: ChatMessage = { role: "assistant", content: response, streaming: false };
      setChatMessages((prev) => [...prev, assistantMsg]);
    } catch (e) {
      const errorMsg: ChatMessage = {
        role: "assistant",
        content: `AI error: ${e}`,
        streaming: false,
      };
      setChatMessages((prev) => [...prev, errorMsg]);
    }
    setIsAiThinking(false);
  }, [isAiThinking, activeFile]);

  return (
    <div className="layout">
      {/* Title Bar */}
      <div className="title-bar">
        <span className="title-bar-brand">Aurora</span>
        <button className="title-bar-menu" onClick={() => {}}>File</button>
        <button className="title-bar-menu" onClick={() => {}}>Edit</button>
        <button className="title-bar-menu" onClick={() => {}}>View</button>
        <button className="title-bar-menu" onClick={() => {}}>AI</button>
      </div>

      <div className="layout-body">
        {/* Activity Bar */}
        <ActivityBar
          activeView={activeSidebar}
          onViewChange={(view) => {
            if (view === "ai") {
              setShowChat((prev) => !prev);
            } else {
              if (activeSidebar === view) {
                setShowSidebar((prev) => !prev);
              } else {
                setActiveSidebar(view);
                setShowSidebar(true);
              }
            }
          }}
        />

        {/* Sidebar */}
        {showSidebar && (
          <Sidebar
            view={activeSidebar}
            fileTree={fileTree}
            activeFilePath={activeFile?.path || ""}
            onOpenFile={handleOpenFile}
            onClose={setShowSidebar}
            workspacePath={workspacePath}
            gitBranch={gitBranch}
            gitRefreshTrigger={gitRefreshTrigger}
          />
        )}

        {/* Center: Editor area + Status Bar */}
        <div className="layout-center">
          {/* Tab Bar */}
          {openFiles.length > 0 && (
            <div className="tab-bar">
              {openFiles.map((file) => (
                <button
                  key={file.path}
                  className={`tab ${activeFile?.path === file.path ? "active" : ""}`}
                  onClick={() => handleSwitchFile(file)}
                >
                  <span className="tab-icon">{fileIcon(file.name)}</span>
                  <span className="tab-name">
                    {file.modified && <span className="tab-modified">● </span>}
                    {file.name}
                  </span>
                  <span
                    className="tab-close"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleCloseFile(file.path);
                    }}
                  >
                    ×
                  </span>
                </button>
              ))}
            </div>
          )}

          {/* Editor */}
          <div className="editor-container">
            {activeFile ? (
              <Editor
                file={activeFile}
                gitRoot={gitRoot}
                onContentChange={(content) => {
                  setOpenFiles((prev) =>
                    prev.map((f) =>
                      f.path === activeFile.path
                        ? { ...f, content, modified: content !== f.content }
                        : f
                    )
                  );
                  // Debounced git status auto-refresh on file edits
                  if (refreshTimerRef.current) {
                    clearTimeout(refreshTimerRef.current);
                  }
                  refreshTimerRef.current = setTimeout(() => {
                    setGitRefreshTrigger((prev) => prev + 1);
                  }, 500);
                }}
                onSave={handleSave}
              />
            ) : (
              <div className="welcome-screen">
                <div className="welcome-title">✦ Aurora</div>
                <div className="welcome-subtitle">
                  Open a file or folder to start editing
                </div>
                <div className="welcome-shortcuts">
                  Ctrl+O — Open File · Ctrl+Shift+O — Open Folder
                </div>
              </div>
            )}
          </div>

          {/* Status Bar */}
          <StatusBar
            activeFile={activeFile}
            gitBranch={gitBranch}
            isAiThinking={isAiThinking}
            aiStatus={isAiThinking ? "Thinking..." : "AI Ready"}
            watcherActive={watcherActive}
            workspacePath={workspacePath}
          />
        </div>

        {/* Chat Panel */}
        {showChat && (
          <ChatPanel
            messages={chatMessages}
            isThinking={isAiThinking}
            onSend={handleSendChat}
            onClose={() => setShowChat(false)}
          />
        )}
      </div>
    </div>
  );
}


