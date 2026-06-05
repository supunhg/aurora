import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ApiKeyInfo } from "../types";
import Icon from "./Icon";

interface Props {
  onClose: (show: boolean) => void;
}

const PROVIDERS = [
  { id: "groq", name: "Groq", hint: "gsk_..." },
  { id: "openai", name: "OpenAI", hint: "sk-..." },
  { id: "gemini", name: "Google Gemini", hint: "AIzaSy..." },
  { id: "anthropic", name: "Anthropic", hint: "sk-ant-..." },
  { id: "cerebras", name: "Cerebras", hint: "csk-..." },
];

const STATUS_CONFIG: Record<string, { color: string; icon: string; label: string }> = {
  healthy: { color: "var(--accent-green)", icon: "material-symbols:check-circle", label: "Healthy" },
  approaching: { color: "var(--accent-yellow)", icon: "material-symbols:warning", label: "Approaching" },
  rate_limited: { color: "var(--accent-orange)", icon: "material-symbols:timer", label: "Rate Limited" },
  invalid: { color: "var(--accent-red)", icon: "material-symbols:error", label: "Invalid" },
  unknown: { color: "var(--text-muted)", icon: "material-symbols:help", label: "Unknown" },
};

export default function KeyManagerPanel({ onClose }: Props) {
  const [keys, setKeys] = useState<ApiKeyInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [showAddForm, setShowAddForm] = useState(false);
  const [selectedProvider, setSelectedProvider] = useState("groq");
  const [keyValue, setKeyValue] = useState("");
  const [keyLabel, setKeyLabel] = useState("");
  const [testingKey, setTestingKey] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  const loadKeys = useCallback(async () => {
    setLoading(true);
    try {
      const data = await invoke<ApiKeyInfo[]>("list_api_keys");
      setKeys(data);
    } catch (e) {
      console.error("Failed to list API keys:", e);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    loadKeys();
  }, [loadKeys]);

  const handleAddKey = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!keyValue.trim()) return;

    setTestingKey(true);
    setTestResult(null);

    try {
      const isValid = await invoke<boolean>("test_api_key", {
        provider: selectedProvider,
        apiKey: keyValue.trim(),
      });

      if (!isValid) {
        setTestResult({ ok: false, msg: "Key format looks invalid for this provider." });
        setTestingKey(false);
        return;
      }

      const label = keyLabel.trim() || `${selectedProvider} key`;
      await invoke("add_api_key", {
        provider: selectedProvider,
        apiKey: keyValue.trim(),
        label,
      });

      setTestResult({ ok: true, msg: "Key added successfully." });
      setKeyValue("");
      setKeyLabel("");
      setShowAddForm(false);
      await loadKeys();
    } catch (err) {
      setTestResult({ ok: false, msg: String(err) });
    }
    setTestingKey(false);
  };

  const handleDelete = async (keyId: string) => {
    try {
      await invoke("delete_api_key", { keyId });
      setDeleteConfirm(null);
      await loadKeys();
    } catch (e) {
      console.error("Failed to delete key:", e);
    }
  };

  const getProviderName = (id: string) =>
    PROVIDERS.find((p) => p.id === id)?.name || id;

  const grouped = keys.reduce<Record<string, ApiKeyInfo[]>>((acc, k) => {
    (acc[k.provider] ||= []).push(k);
    return acc;
  }, {});

  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-header-title">AI Key Manager</span>
        <div className="sidebar-header-actions">
          <button
            className="sidebar-header-btn"
            onClick={() => {
              setShowAddForm((s) => !s);
              setTestResult(null);
            }}
            title="Add key"
          >
            <Icon icon="material-symbols:add" size={14} />
          </button>
          <button
            className="sidebar-header-btn"
            onClick={() => onClose(false)}
            title="Collapse sidebar"
          >
            −
          </button>
        </div>
      </div>

      <div className="sidebar-content">
        {/* Add Key Form */}
        {showAddForm && (
          <form className="km-form" onSubmit={handleAddKey}>
            <div className="km-form-row">
              <label className="km-form-label">Provider</label>
              <select
                className="km-select"
                value={selectedProvider}
                onChange={(e) => setSelectedProvider(e.target.value)}
              >
                {PROVIDERS.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </div>
            <div className="km-form-row">
              <label className="km-form-label">Label</label>
              <input
                className="km-input"
                type="text"
                placeholder="e.g. personal-work"
                value={keyLabel}
                onChange={(e) => setKeyLabel(e.target.value)}
              />
            </div>
            <div className="km-form-row">
              <label className="km-form-label">API Key</label>
              <input
                className="km-input"
                type="password"
                placeholder={
                  PROVIDERS.find((p) => p.id === selectedProvider)?.hint || "sk-..."
                }
                value={keyValue}
                onChange={(e) => setKeyValue(e.target.value)}
                required
              />
            </div>
            {testResult && (
              <div
                className={`km-test-result ${testResult.ok ? "success" : "error"}`}
              >
                <Icon
                  icon={
                    testResult.ok
                      ? "material-symbols:check-circle"
                      : "material-symbols:error"
                  }
                  size={12}
                />
                {testResult.msg}
              </div>
            )}
            <div className="km-form-actions">
              <button
                type="button"
                className="km-btn km-btn-secondary"
                onClick={() => {
                  setShowAddForm(false);
                  setTestResult(null);
                }}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="km-btn km-btn-primary"
                disabled={testingKey || !keyValue.trim()}
              >
                {testingKey ? "Testing..." : "Add & Test"}
              </button>
            </div>
          </form>
        )}

        {/* Keys List */}
        {loading && keys.length === 0 && (
          <div className="sidebar-placeholder">
            <div className="sidebar-placeholder-text">Loading keys...</div>
          </div>
        )}

        {keys.length === 0 && !loading && (
          <div className="sidebar-placeholder">
            <div className="sidebar-placeholder-icon">
              <Icon icon="material-symbols:key-off" size={28} />
            </div>
            <div className="sidebar-placeholder-text">No API keys configured</div>
            <div className="sidebar-placeholder-text" style={{ fontSize: 11 }}>
              Add keys to enable AI provider rotation
            </div>
          </div>
        )}

        {Object.entries(grouped).map(([provider, providerKeys]) => (
          <div key={provider} className="km-provider-section">
            <div className="km-provider-header">
              <span className="km-provider-name">
                {getProviderName(provider)}
              </span>
              <span className="km-provider-count">
                {providerKeys.length} key{providerKeys.length !== 1 ? "s" : ""}
              </span>
            </div>
            <div className="km-keys-list">
              {providerKeys.map((key) => {
                const status = STATUS_CONFIG[key.status] || STATUS_CONFIG.unknown;
                return (
                  <div key={key.key_id} className="km-key-item">
                    <div className="km-key-main">
                      <span
                        className="km-key-status-dot"
                        style={{ background: status.color }}
                        title={status.label}
                      />
                      <span className="km-key-label">{key.label}</span>
                    </div>

                    {/* Usage bar */}
                    <div className="km-key-usage">
                      <div className="km-usage-bar-bg">
                        <div
                          className={`km-usage-bar-fill ${
                            key.percent_used >= 95
                              ? "critical"
                              : key.percent_used >= 80
                                ? "warning"
                                : "healthy"
                          }`}
                          style={{ width: `${Math.min(key.percent_used, 100)}%` }}
                        />
                      </div>
                      <span className="km-usage-text">
                        {key.percent_used.toFixed(0)}%
                      </span>
                    </div>

                    <div className="km-key-meta">
                      <span className="km-key-id" title={key.key_id}>
                        {key.key_id.slice(0, 12)}…
                      </span>
                      {deleteConfirm === key.key_id ? (
                        <div className="km-delete-confirm">
                          <button
                            className="km-btn km-btn-danger km-btn-small"
                            onClick={() => handleDelete(key.key_id)}
                          >
                            Confirm
                          </button>
                          <button
                            className="km-btn km-btn-secondary km-btn-small"
                            onClick={() => setDeleteConfirm(null)}
                          >
                            Cancel
                          </button>
                        </div>
                      ) : (
                        <button
                          className="km-key-delete"
                          onClick={() => setDeleteConfirm(key.key_id)}
                          title="Delete key"
                        >
                          <Icon icon="material-symbols:delete" size={12} />
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
