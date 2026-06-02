export function fileIcon(filename: string): string {
  if (filename.endsWith(".rs")) return "R";
  if (filename.endsWith(".ts") || filename.endsWith(".tsx")) return "T";
  if (filename.endsWith(".js") || filename.endsWith(".jsx")) return "J";
  if (filename.endsWith(".py")) return "P";
  if (filename.endsWith(".go")) return "G";
  if (filename.endsWith(".json")) return "{";
  if (filename.endsWith(".yaml") || filename.endsWith(".yml")) return "Y";
  if (filename.endsWith(".md")) return "M";
  if (filename.endsWith(".html")) return "H";
  if (filename.endsWith(".css") || filename.endsWith(".scss")) return "#";
  if (filename === "Cargo.toml" || filename === "package.json") return "⚙";
  if (filename.endsWith(".toml")) return "T";
  return "·";
}

export function fileIconColor(filename: string): string {
  if (filename.endsWith(".rs") || filename.endsWith(".ts") || filename.endsWith(".tsx")) return "var(--accent-blue)";
  if (filename.endsWith(".js") || filename.endsWith(".jsx") || filename.endsWith(".toml") || filename.endsWith(".json")) return "var(--accent-yellow)";
  if (filename.endsWith(".py")) return "var(--accent-green)";
  if (filename.endsWith(".go") || filename.endsWith(".html")) return "var(--accent-cyan)";
  if (filename.endsWith(".yaml") || filename.endsWith(".yml")) return "var(--accent-red)";
  if (filename.endsWith(".md")) return "var(--accent-blue)";
  if (filename.endsWith(".css") || filename.endsWith(".scss")) return "var(--accent-purple)";
  return "var(--text-secondary)";
}
