import { useState, useRef, useEffect } from "react";
import type { ChatMessage } from "../types";
import Icon from "./Icon";

interface Props {
  messages: ChatMessage[];
  isThinking: boolean;
  onSend: (message: string) => void;
  onClose: () => void;
}

export default function ChatPanel({ messages, isThinking, onSend, onClose }: Props) {
  const [input, setInput] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Auto-scroll to bottom on new messages
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const handleSend = () => {
    if (!input.trim() || isThinking) return;
    onSend(input.trim());
    setInput("");
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="chat-panel">
      {/* Header */}
      <div className="chat-header">
        <span className="chat-header-title">
          <Icon icon="material-symbols:auto-awesome" size={14} style={{ marginRight: 4, verticalAlign: "middle" }} />
          Chat
        </span>
        <button className="chat-header-btn" onClick={() => setInput("")} title="Clear chat">
          <Icon icon="material-symbols:delete-outline" size={14} />
        </button>
        <button className="chat-header-btn" onClick={onClose} title="Close panel">
          <Icon icon="material-symbols:close" size={14} />
        </button>
      </div>

      {/* Messages */}
      <div className="chat-messages">
        {messages.length === 0 && !isThinking && (
          <div
            style={{
              textAlign: "center",
              color: "var(--text-muted)",
              padding: "40px 20px",
              fontSize: 12,
            }}
          >
            Ask Aurora something to get started.
          </div>
        )}
        {messages.map((msg, i) => (
          <div key={i} className={`chat-message ${msg.role}`}>
            <div className={`chat-message-role ${msg.role}`}>
              {msg.role === "user" ? "You" : msg.role === "assistant" ? "Aurora" : "System"}
            </div>
            <div className="chat-message-content">{msg.content}</div>
          </div>
        ))}
        {isThinking && (
          <div className="chat-thinking">
            <Icon icon="material-symbols:progress-activity" size={12} style={{ marginRight: 4, verticalAlign: "middle" }} />
            Thinking...
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Input */}
      <div className="chat-input-area">
        <div className="chat-input-wrapper">
          <textarea
            ref={inputRef}
            className="chat-input"
            placeholder="Ask Aurora something..."
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            rows={1}
          />
          <button
            className={`chat-send-btn ${input.trim() ? "active" : ""}`}
            onClick={handleSend}
            title="Send"
          >
            <Icon icon="material-symbols:arrow-upward" size={18} />
          </button>
        </div>
      </div>
    </div>
  );
}
