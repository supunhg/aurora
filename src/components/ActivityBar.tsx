import type { SidebarView } from "../types";
import Icon from "./Icon";

interface Props {
  activeView: SidebarView;
  onViewChange: (view: SidebarView) => void;
}

const items: { view: SidebarView; icon: string; label: string }[] = [
  { view: "explorer", icon: "material-symbols:folder-outline", label: "Explorer" },
  { view: "search", icon: "material-symbols:search", label: "Search" },
  { view: "source-control", icon: "material-symbols:call-split", label: "Source Control" },
  { view: "debug", icon: "material-symbols:play-arrow", label: "Run and Debug" },
  { view: "extensions", icon: "material-symbols:extension", label: "Extensions" },
  { view: "ai", icon: "material-symbols:auto-awesome", label: "AI Chat" },
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
          <Icon icon={item.icon} size={20} />
        </button>
      ))}
      <div className="activity-bar-spacer" />
    </div>
  );
}
