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

export type SidebarView = "explorer" | "search" | "source-control" | "debug" | "extensions" | "ai" | "keys";

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

// ---------------------------------------------------------------------------
// Key Management Types
// ---------------------------------------------------------------------------

export interface ApiKeyInfo {
  key_id: string;
  provider: string;
  label: string;
  source: "env" | "config" | "ui";
  status: "healthy" | "approaching" | "rate_limited" | "invalid" | "unknown";
  percent_used: number;
  rpm_used: number;
  rpm_limit: number;
  last_used?: string;
}

export interface KeyUsageInfo {
  provider: string;
  key_id: string;
  label: string;
  rpm_used: number;
  rpm_limit: number;
  rpd_used: number;
  rpd_limit: number;
  tpm_used: number;
  tpm_limit: number;
  tpd_used: number;
  tpd_limit: number;
  percent_used: number;
  status: string;
}

export interface KeyRotationEventPayload {
  event_type: "key_rotated" | "key_approaching" | "key_critical" | "provider_exhausted" | "all_exhausted" | "usage_updated" | "key_added" | "key_removed" | "key_invalidated" | "key_recovered";
  provider?: string;
  from_key_id?: string;
  to_key_id?: string;
  key_id?: string;
  key_label?: string;
  percent_used?: number;
  dimension?: string;
  reason?: string;
}

export interface AiKeyStatus {
  provider: string;
  model: string;
  key_label: string;
  percent_used: number;
  dimension: string;
  state: "healthy" | "approaching" | "critical" | "exhausted";
}

