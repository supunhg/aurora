import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SearchResult } from "../types";
import { fileIcon, fileIconColor } from "../utils/icons";

interface Props {
  workspacePath: string;
  onOpenFile: (path: string) => void;
  onClose: (show: boolean) => void;
}

export default function SearchSidebar({
  workspacePath,
  onOpenFile,
  onClose,
}: Props) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [searched, setSearched] = useState(false);
  const [collapsedPaths, setCollapsedPaths] = useState<Set<string>>(new Set());

  // Group results by file path
  const groupedResults: Map<string, SearchResult[]> = new Map();
  for (const r of results) {
    const existing = groupedResults.get(r.path) || [];
    existing.push(r);
    groupedResults.set(r.path, existing);
  }

  const handleSearch = useCallback(async () => {
    if (!query.trim() || !workspacePath) return;
    setSearching(true);
    setSearched(true);
    try {
      const root = await invoke<string>("get_git_root", { path: workspacePath });
      const searchResults = await invoke<SearchResult[]>("search_files", {
        path: root,
        query: query.trim(),
        maxResults: 200,
      });
      setResults(searchResults);
    } catch (e) {
      console.error("Search failed:", e);
      setResults([]);
    }
    setSearching(false);
  }, [query, workspacePath]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      handleSearch();
    }
    if (e.key === "Escape") {
      setQuery("");
      setResults([]);
      setSearched(false);
    }
  };

  const toggleCollapse = (path: string) => {
    setCollapsedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  const totalMatches = results.length;
  const fileCount = groupedResults.size;

  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-header-title">Search</span>
        <button className="sidebar-header-btn" onClick={() => onClose(false)} title="Collapse sidebar">
          −
        </button>
      </div>

      {/* Search input */}
      <div className="search-input-area">
        <div className="search-input-wrapper">
          <span className="search-input-icon">🔍</span>
          <input
            className="search-input"
            type="text"
            placeholder="Search files..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            autoFocus
          />
          {query && (
            <button
              className="search-clear-btn"
              onClick={() => {
                setQuery("");
                setResults([]);
                setSearched(false);
              }}
            >
              ×
            </button>
          )}
        </div>
        <button
          className="search-btn"
          onClick={handleSearch}
          disabled={!query.trim() || searching}
        >
          {searching ? "Searching..." : "Search"}
        </button>
      </div>

      {/* Results summary */}
      {searched && !searching && (
        <div className="search-summary">
          {totalMatches > 0
            ? `${totalMatches} results in ${fileCount} files`
            : "No results found"}
        </div>
      )}

      {/* Results */}
      <div className="sidebar-content search-results-list">
        {searching && (
          <div className="search-placeholder">
            <div className="search-placeholder-text">Searching...</div>
          </div>
        )}

        {!searching && !searched && (
          <div className="search-placeholder">
            <div className="search-placeholder-icon">🔍</div>
            <div className="search-placeholder-text">Search across files</div>
            <div className="search-placeholder-detail">
              Enter a search term and press Enter
            </div>
          </div>
        )}

        {!searching && searched && totalMatches === 0 && (
          <div className="search-placeholder">
            <div className="search-placeholder-icon">✕</div>
            <div className="search-placeholder-text">No results found</div>
          </div>
        )}

        {!searching && groupedResults.size > 0 && (
          <>
            {Array.from(groupedResults.entries()).map(([filePath, fileResults]) => {
              const collapsed = collapsedPaths.has(filePath);
              return (
                <div key={filePath} className="search-file-group">
                  <div
                    className="search-file-header"
                    onClick={() => toggleCollapse(filePath)}
                  >
                    <span className="search-collapse-arrow">
                      {collapsed ? "▶" : "▼"}
                    </span>
                    <span
                      className="search-file-icon"
                      style={{ color: fileIconColor(filePath) }}
                    >
                      {fileIcon(filePath)}
                    </span>
                    <span className="search-file-path">
                      {filePath.split("/").pop() || filePath}
                    </span>
                    <span className="search-match-count">
                      {fileResults.length}
                    </span>
                  </div>
                  {!collapsed && (
                    <div className="search-results-list-inner">
                      {fileResults.map((r, i) => (
                        <div
                          key={`${r.path}-${r.line}-${i}`}
                          className="search-result-item"
                          onClick={() => onOpenFile(r.path)}
                        >
                          <span className="search-result-line">L{r.line}</span>
                          <span className="search-result-text">{r.text}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
          </>
        )}
      </div>
    </div>
  );
}
