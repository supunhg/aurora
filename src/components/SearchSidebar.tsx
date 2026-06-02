import { useState, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SearchResult } from "../types";
import { fileIconColor } from "../utils/icons";
import Icon from "./Icon";

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
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Group results by file path
  const groupedResults: Map<string, SearchResult[]> = new Map();
  for (const r of results) {
    const existing = groupedResults.get(r.path) || [];
    existing.push(r);
    groupedResults.set(r.path, existing);
  }

  const handleSearch = useCallback(async (searchQuery: string) => {
    if (!searchQuery.trim() || !workspacePath) return;
    setSearching(true);
    setSearched(true);
    try {
      const root = await invoke<string>("get_git_root", { path: workspacePath });
      const searchResults = await invoke<SearchResult[]>("search_files", {
        path: root,
        query: searchQuery.trim(),
        maxResults: 200,
      });
      setResults(searchResults);
    } catch (e) {
      console.error("Search failed:", e);
      setResults([]);
    }
    setSearching(false);
  }, [workspacePath]);

  // Debounced auto-search: fires 300ms after user stops typing
  useEffect(() => {
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }
    if (query.trim().length >= 2) {
      debounceRef.current = setTimeout(() => {
        handleSearch(query);
      }, 300);
    } else {
      setResults([]);
      setSearched(false);
    }
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [query, handleSearch]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      handleSearch(query);
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
          <Icon icon="material-symbols:close" size={14} />
        </button>
      </div>

      {/* Search input */}
      <div className="search-input-area">
        <div className="search-input-wrapper">
          <span className="search-input-icon">
            <Icon icon="material-symbols:search" size={14} />
          </span>
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
              <Icon icon="material-symbols:close" size={14} />
            </button>
          )}
        </div>
        {searching && (
          <div className="search-hint">Searching...</div>
        )}
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
        {!searched && !searching && query.length === 0 && (
          <div className="search-placeholder">
            <div className="search-placeholder-icon">
              <Icon icon="material-symbols:search" size={28} />
            </div>
            <div className="search-placeholder-text">Search across files</div>
            <div className="search-placeholder-detail">
              Start typing to search — results update automatically
            </div>
          </div>
        )}

        {searched && totalMatches === 0 && !searching && (
          <div className="search-placeholder">
            <div className="search-placeholder-icon">
              <Icon icon="material-symbols:search-off" size={28} />
            </div>
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
                      style={{ color: fileIconColor(filePath), display: "flex", alignItems: "center" }}
                    >
                      <Icon icon="material-symbols:description" size={14} />
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
