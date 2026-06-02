import type { SidebarView } from "../types";

interface Props {
  activeView: SidebarView;
  onViewChange: (view: SidebarView) => void;
}

const items: { view: SidebarView; icon: string; label: string }[] = [
  { view: "explorer", icon: "📁", label: "Explorer" },
  { view: "search", icon: "🔍", label: "Search" },
  { view: "source-control", icon: "⎇", label: "Source Control" },
  { view: "debug", icon: "▶", label: "Run and Debug" },
  { view: "extensions", icon: "◆", label: "Extensions" },
  { view: "ai", icon: "✦", label: "AI Chat" },
];

export default function ActivityBar({ activeView, onViewChange }: Props) {
  return (
    <div className="activity-bar">
      {items.map((item) => (
        <button
          key={item.view}
          className={`activity-bar-item ${activeView === item.view ? "active" : ""}`}
          onClick={() => onViewChange(item.view)}
          title={item.label}
        >
          {item.icon}
        </button>
      ))}
      <div className="activity-bar-spacer" />
    </div>
  );
}
