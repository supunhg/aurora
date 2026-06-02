import { useRef, useCallback } from "react";
import Editor, { OnMount, OnChange } from "@monaco-editor/react";
import type { OpenFile } from "../types";

interface Props {
  file: OpenFile;
  onContentChange: (content: string) => void;
  onSave?: (path: string, content: string) => void;
}

const languageMap: Record<string, string> = {
  rust: "rust",
  typescript: "typescript",
  javascript: "javascript",
  python: "python",
  go: "go",
  json: "json",
  toml: "plaintext",
  yaml: "yaml",
  markdown: "markdown",
  html: "html",
  css: "css",
  c: "c",
  cpp: "cpp",
  java: "java",
  ruby: "ruby",
  php: "php",
  shell: "shell",
  sql: "sql",
  lua: "lua",
  dart: "dart",
  swift: "swift",
  kotlin: "kotlin",
  plaintext: "plaintext",
};

export default function MonacoEditor({ file, onContentChange, onSave }: Props) {
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);

  const handleEditorDidMount: OnMount = useCallback((editor) => {
    editorRef.current = editor;

    // Register Ctrl+S command
    editor.addCommand(
      // KeyMod.CtrlCmd | KeyCode.KeyS
      2048 | 49, // CtrlCmd + S
      () => {
        if (onSave) {
          const content = editor.getValue();
          onSave(file.path, content);
        }
      }
    );

    editor.focus();
  }, [file.path, onSave]);

  const handleChange: OnChange = useCallback(
    (value) => {
      if (value !== undefined) {
        onContentChange(value);
      }
    },
    [onContentChange]
  );

  const language = languageMap[file.language] || "plaintext";

  return (
    <div className="monaco-editor-container">
      <Editor
        height="100%"
        language={language}
        value={file.content}
        theme="aurora-dark"
        onChange={handleChange}
        onMount={handleEditorDidMount}
        options={{
          fontSize: 13,
          fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', 'Consolas', monospace",
          lineHeight: 19,
          minimap: { enabled: true, scale: 1 },
          scrollBeyondLastLine: false,
          wordWrap: "off",
          tabSize: 4,
          insertSpaces: true,
          cursorBlinking: "smooth",
          cursorSmoothCaretAnimation: "on",
          smoothScrolling: true,
          padding: { top: 4 },
          renderLineHighlight: "line",
          lineNumbers: "on",
          glyphMargin: false,
          folding: true,
          bracketPairColorization: { enabled: true },
          autoClosingBrackets: "always",
          autoClosingQuotes: "always",
          formatOnPaste: true,
          renderWhitespace: "selection",
          renderControlCharacters: false,
          matchBrackets: "always",
          selectionHighlight: true,
          overviewRulerBorder: false,
          hideCursorInOverviewRuler: true,
          overviewRulerLanes: 0,
          // VS Code-like behavior
          mouseWheelZoom: true,
          // Performance
          suggest: { showKeywords: true, showSnippets: true },
        }}
        beforeMount={(monaco) => {
          // Define Aurora dark theme
          monaco.editor.defineTheme("aurora-dark", {
            base: "vs-dark",
            inherit: true,
            rules: [
              { token: "comment", foreground: "565e78", fontStyle: "italic" },
              { token: "keyword", foreground: "cba6f7" },
              { token: "string", foreground: "9ece6a" },
              { token: "number", foreground: "ff9e64" },
              { token: "type", foreground: "2ac3de" },
              { token: "function", foreground: "7dcfff" },
              { token: "variable", foreground: "a9b1d6" },
              { token: "constant", foreground: "ff9e64" },
              { token: "operator", foreground: "89dceb" },
            ],
            colors: {
              "editor.background": "#1a1b26",
              "editor.foreground": "#a9b1d6",
              "editor.lineHighlightBackground": "#202232",
              "editor.selectionBackground": "#364a82",
              "editor.inactiveSelectionBackground": "#364a8250",
              "editorCursor.foreground": "#c0caf5",
              "editorLineNumber.foreground": "#3b4261",
              "editorLineNumber.activeForeground": "#7880aa",
              "editor.selectionHighlightBackground": "#364a8240",
              "editorBracketMatch.background": "#364a8230",
              "editorBracketMatch.border": "#89b4fa",
              "editorGutter.background": "#181924",
              "editorGutter.modifiedBackground": "#f9e2af",
              "editorGutter.addedBackground": "#a6e3a1",
              "editorGutter.deletedBackground": "#f38ba8",
              "minimap.background": "#1a1b26",
              "scrollbar.shadow": "#00000000",
              "scrollbarSlider.background": "#30344980",
              "scrollbarSlider.hoverBackground": "#303449",
              "scrollbarSlider.activeBackground": "#565e78",
              "editorWidget.background": "#24283b",
              "editorWidget.border": "#303449",
              "input.background": "#2a2e42",
              "input.foreground": "#a9b1d6",
              "input.border": "#303449",
              "list.activeSelectionBackground": "#1c1e2e",
              "list.hoverBackground": "#232638",
              "list.inactiveSelectionBackground": "#1c1e2e",
              "editorOverviewRuler.background": "#1a1b26",
              "tab.activeBackground": "#1a1b26",
              "tab.inactiveBackground": "#222537",
              "tab.activeBorder": "#89b4fa",
            },
          });
        }}
      />
    </div>
  );
}
