export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  depth: number;
  expanded: boolean;
}

export interface ChatMessage {
  role: "user" | "assistant" | "system";
  content: string;
  streaming: boolean;
}

export type SidebarView = "explorer" | "search" | "source-control" | "debug" | "extensions" | "ai";

export interface OpenFile {
  name: string;
  path: string;
  content: string;
  modified: boolean;
  language: string;
}

export interface GitFileStatus {
  path: string;
  status: string;
  staged: boolean;
  original_path: string | null;
}

export interface GitCommitInfo {
  hash: string;
  message: string;
  author: string;
}

export interface GitBranchInfo {
  name: string;
  current: boolean;
  upstream: string | null;
}

export interface GitStashEntry {
  index: number;
  message: string;
}

export interface SearchResult {
  path: string;
  line: number;
  text: string;
}

export interface FileWatchEvent {
  kind: "created" | "modified" | "deleted";
  path: string;
}

export interface GutterDecoration {
  line_number: number;
  kind: "modified" | "added" | "deleted";
}

