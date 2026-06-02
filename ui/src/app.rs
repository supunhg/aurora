use crate::theme;
use editor::Editor;
use eframe::egui::{self, Color32, Frame, Margin, Rect, Rounding, Stroke, Vec2};
use std::sync::mpsc;

/// A message in the AI chat panel.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub streaming: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// Which view is shown in the sidebar (like VS Code's activity bar views).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarView {
    Explorer,
    Search,
    SourceControl,
    Debug,
    Extensions,
    AI,
}

/// The main Aurora application state.
pub struct AuroraApp {
    pub editor: Editor,
    pub chat_messages: Vec<ChatMessage>,
    pub chat_input: String,
    pub agent_trace: Vec<String>,
    pub file_tree: Vec<FileEntry>,
    pub open_files: Vec<OpenFile>,
    pub active_tab: usize,
    pub status_text: String,
    pub ai_status: String,
    pub show_agent_panel: bool,
    pub show_chat_panel: bool,
    pub workspace_root: Option<std::path::PathBuf>,
    /// The currently active sidebar view (Explorer, Search, etc.)
    pub active_sidebar: SidebarView,
    /// Whether to show the sidebar
    pub show_sidebar: bool,
    /// Channel for receiving AI responses from background tasks
    ai_rx: Option<mpsc::Receiver<String>>,
    /// Handle to spawn async tasks
    runtime: tokio::runtime::Handle,
    /// Whether we're waiting for an AI response
    waiting_for_response: bool,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: std::path::PathBuf,
    pub is_dir: bool,
    pub depth: usize,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct OpenFile {
    pub name: String,
    pub path: std::path::PathBuf,
    pub content: String,
    pub modified: bool,
}

impl Default for AuroraApp {
    fn default() -> Self {
        Self {
            editor: Editor::new(),
            chat_messages: Vec::new(),
            chat_input: String::new(),
            agent_trace: Vec::new(),
            file_tree: Vec::new(),
            open_files: Vec::new(),
            active_tab: 0,
            status_text: "Ready".into(),
            ai_status: "AI Ready".into(),
            show_agent_panel: false,
            show_chat_panel: true,
            active_sidebar: SidebarView::Explorer,
            show_sidebar: true,
            workspace_root: None,
            ai_rx: None,
            runtime: tokio::runtime::Handle::current(),
            waiting_for_response: false,
        }
    }
}

impl AuroraApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    /// Open a file in the editor.
    pub fn open_file(&mut self, path: &std::path::Path) {
        if let Ok(text) = std::fs::read_to_string(path) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "untitled".into());

            // Check if file is already open
            if let Some(pos) = self.open_files.iter().position(|f| f.path == path) {
                self.active_tab = pos;
                let content = self.open_files[pos].content.clone();
                self.editor.load_text(&content);
                self.status_text = format!("Switched to {}", name);
                return;
            }

            self.open_files.push(OpenFile {
                name: name.clone(),
                path: path.to_path_buf(),
                content: text.clone(),
                modified: false,
            });
            self.active_tab = self.open_files.len() - 1;
            self.editor.load_text(&text);

            // Detect language for highlighting
            let keywords: &[&str] = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => &editor::RUST_KEYWORDS,
                Some("ts") | Some("tsx") | Some("js") | Some("jsx") => &editor::TYPESCRIPT_KEYWORDS,
                Some("py") => &editor::PYTHON_KEYWORDS,
                _ => &[],
            };
            self.editor.highlight_visible_range(keywords);
            self.editor.file_path = Some(path.to_path_buf());
            self.status_text = format!("Opened {}", name);
        }
    }

    /// Open a directory and populate the file tree.
    pub fn open_directory(&mut self, path: &std::path::Path) {
        self.workspace_root = Some(path.to_path_buf());
        self.file_tree.clear();
        self.build_file_tree(path, 0);
        self.status_text = format!("Opened {}", path.display());
    }

    fn build_file_tree(&mut self, dir: &std::path::Path, depth: usize) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            sorted.sort_by(|a, b| {
                let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
                b_dir
                    .cmp(&a_dir)
                    .then_with(|| a.file_name().cmp(&b.file_name()))
            });

            for entry in sorted {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                self.file_tree.push(FileEntry {
                    name,
                    path: entry.path(),
                    is_dir,
                    depth,
                    expanded: false,
                });
            }
        }
    }

    #[allow(dead_code)]
    fn save_current_file(&mut self) {
        if let Some(file) = self.open_files.get_mut(self.active_tab) {
            let content = self.editor.buffer.text();
            if std::fs::write(&file.path, &content).is_ok() {
                file.content = content;
                file.modified = false;
                self.status_text = format!("Saved {}", file.name);
            }
        }
    }

    fn close_tab(&mut self, idx: usize) {
        if idx < self.open_files.len() {
            self.open_files.remove(idx);
            if self.active_tab >= self.open_files.len() && !self.open_files.is_empty() {
                self.active_tab = self.open_files.len() - 1;
            }
            if !self.open_files.is_empty() {
                let content = self.open_files[self.active_tab].content.clone();
                self.editor.load_text(&content);
            }
        }
    }

    /// Send a chat message to the AI backend.
    fn send_chat_message(&mut self, message: String) {
        self.chat_messages.push(ChatMessage {
            role: MessageRole::User,
            content: message.clone(),
            streaming: false,
        });

        self.waiting_for_response = true;
        self.status_text = "Thinking...".into();

        let mut context = String::new();
        if let Some(file) = self.open_files.get(self.active_tab) {
            context.push_str(&format!("Currently editing: {}\n\n", file.name));
            context.push_str("File contents:\n");
            context.push_str(&file.content);
        }
        if let Some(ref root) = self.workspace_root {
            context.push_str(&format!("\n\nWorkspace: {}", root.display()));
        }

        let (tx, rx) = mpsc::channel();
        self.ai_rx = Some(rx);

        let prompt = message.clone();
        let model = "auto".to_string();

        self.runtime.spawn(async move {
            let client = ai::FreeLlmClient::localhost();
            let health = client.health_check().await;

            let response = if health {
                let messages = vec![
                    ai::freellm::ChatMessage {
                        role: "system".into(),
                        content: "You are Aurora, an AI coding assistant. Help with software engineering tasks. Be concise and helpful.".into(),
                    },
                    ai::freellm::ChatMessage {
                        role: "user".into(),
                        content: if context.is_empty() {
                            prompt
                        } else {
                            format!("{}\n\nUser request: {}", context, prompt)
                        },
                    },
                ];

                match client.chat_completion(&model, messages).await {
                    Ok(resp) => {
                        resp.choices
                            .first()
                            .and_then(|c| c.message.content.clone())
                            .unwrap_or_else(|| "No response from AI".into())
                    }
                    Err(e) => format!("AI error: {}", e),
                }
            } else {
                format!(
                    "The AI sidecar is not running. To enable AI chat:\n\n\
                     1. Run: `sidecar/setup.sh` (installs FreeLLMAPI)\n\
                     2. Start: `cd sidecar/freellmapi && npm run dev`\n\
                     3. The AI will then be available at localhost:3001\n\n\
                     Your message was: \"{}\"",
                    prompt
                )
            };

            let _ = tx.send(response);
        });
    }

    // ==================================================================
    // Layout: Title Bar
    // ==================================================================

    fn render_title_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("title_bar")
            .frame(theme::title_bar_frame())
            .min_height(30.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Brand
                    ui.label(
                        egui::RichText::new("Aurora")
                            .color(theme::ACCENT_BLUE)
                            .size(13.0)
                            .strong(),
                    );
                    ui.add_space(12.0);

                    // Menu buttons
                    let menu_style = egui::RichText::new("  File  ")
                        .color(theme::TEXT_SECONDARY)
                        .size(12.0);
                    ui.menu_button(menu_style, |ui| {
                        ui.set_min_width(180.0);
                        let resp = ui.button(
                            egui::RichText::new("Open File...")
                                .color(theme::TEXT_PRIMARY)
                                .size(13.0),
                        );
                        if resp.clicked() { ui.close_menu(); }
                        ui.label(
                            egui::RichText::new("Ctrl+O")
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                        ui.separator();
                        let _ = ui.button(
                            egui::RichText::new("Open Folder...")
                                .color(theme::TEXT_PRIMARY)
                                .size(13.0),
                        );
                        ui.label(
                            egui::RichText::new("Ctrl+Shift+O")
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                        ui.separator();
                        let _ = ui.button(
                            egui::RichText::new("Save")
                                .color(theme::TEXT_PRIMARY)
                                .size(13.0),
                        );
                        ui.label(
                            egui::RichText::new("Ctrl+S")
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                        ui.separator();
                        let _ = ui.button(
                            egui::RichText::new("Close Tab")
                                .color(theme::TEXT_PRIMARY)
                                .size(13.0),
                        );
                        ui.label(
                            egui::RichText::new("Ctrl+W")
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                        ui.separator();
                        let exit_resp = ui.button(
                            egui::RichText::new("Exit")
                                .color(theme::ACCENT_RED)
                                .size(13.0),
                        );
                        if exit_resp.clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });

                    let view_style = egui::RichText::new("  View  ")
                        .color(theme::TEXT_SECONDARY)
                        .size(12.0);
                    ui.menu_button(view_style, |ui| {
                        ui.set_min_width(160.0);
                        ui.checkbox(&mut self.show_sidebar, "Sidebar");
                        ui.checkbox(&mut self.show_chat_panel, "Chat");
                        ui.checkbox(&mut self.show_agent_panel, "Agent Panel");
                    });

                    let ai_style = egui::RichText::new("  AI  ")
                        .color(theme::ACCENT_PURPLE)
                        .size(12.0);
                    ui.menu_button(ai_style, |ui| {
                        ui.set_min_width(140.0);
                        if ui.button("New Chat").clicked() {
                            self.chat_messages.clear();
                            self.chat_input.clear();
                            ui.close_menu();
                        }
                        if ui.button("Open Agent Panel").clicked() {
                            self.show_agent_panel = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.label(
                            egui::RichText::new(&self.ai_status)
                                .color(theme::STATUS_AI_READY)
                                .size(11.0),
                        );
                    });

                    // Spacer - push file path to center-ish area
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                    });
                });
            });
    }

    // ==================================================================
    // Layout: Activity Bar (VS Code left icon strip)
    // ==================================================================

    fn render_activity_bar(&mut self, ui: &mut egui::Ui) {
        let item_size = Vec2::new(48.0, 48.0);
        let bar_height = ui.available_height();

        // Center the items vertically in the available space
        let total_items = 6;
        let content_height = total_items as f32 * 48.0;
        let top_offset = ((bar_height - content_height) / 2.0).max(0.0);

        // Draw activity bar vertical section divider
        let bar_rect = ui.max_rect();
        ui.painter().rect_filled(
            Rect::from_min_max(
                egui::pos2(bar_rect.right() - 1.0, bar_rect.top()),
                egui::pos2(bar_rect.right(), bar_rect.bottom()),
            ),
            Rounding::default(),
            theme::BORDER_SECTION,
        );

        ui.add_space(top_offset);

        let items = [
            (SidebarView::Explorer, "📁", "Explorer"),
            (SidebarView::Search, "🔍", "Search"),
            (SidebarView::SourceControl, "⎇", "Source Control"),
            (SidebarView::Debug, "▶", "Run and Debug"),
            (SidebarView::Extensions, "◆", "Extensions"),
            (SidebarView::AI, "✦", "AI Chat"),
        ];

        for (view, icon, tooltip) in &items {
            let is_active = self.active_sidebar == *view;
            let (response, painter) = ui.allocate_painter(item_size, egui::Sense::click());

            let rect = response.rect;

            // Active indicator: left border
            if is_active {
                // Left accent bar (2px blue)
                painter.rect_filled(
                    Rect::from_min_max(
                        egui::pos2(rect.left(), rect.top()),
                        egui::pos2(rect.left() + 2.0, rect.bottom()),
                    ),
                    Rounding::default(),
                    theme::ACCENT_BLUE,
                );

                // Active background (slightly lighter)
                painter.rect_filled(
                    rect.shrink2(Vec2::new(0.0, 0.0)),
                    Rounding::default(),
                    Color32::from_rgba_premultiplied(255, 255, 255, 8),
                );
            }

            // Hover state
            if response.hovered() && !is_active {
                painter.rect_filled(
                    rect.shrink2(Vec2::new(4.0, 2.0)),
                    Rounding::same(4.0),
                    Color32::from_rgba_premultiplied(255, 255, 255, 12),
                );
            }

            // Icon
            let icon_color = if is_active {
                theme::TEXT_ACTIVITY_BAR
            } else {
                theme::TEXT_ACTIVITY_INACTIVE
            };

            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                *icon,
                egui::FontId::new(18.0, egui::FontFamily::Proportional),
                icon_color,
            );

            // Tooltip
            if response.hovered() {
                let tooltip_rect = Rect::from_min_size(
                    egui::pos2(rect.right() + 4.0, rect.top()),
                    Vec2::new(120.0, 22.0),
                );
                painter.rect_filled(tooltip_rect, Rounding::same(3.0), theme::BG_ACTIVE);
                painter.text(
                    tooltip_rect.center(),
                    egui::Align2::LEFT_CENTER,
                    *tooltip,
                    egui::FontId::new(11.0, egui::FontFamily::Proportional),
                    theme::TEXT_PRIMARY,
                );
            }

            if response.clicked() {
                if *view == SidebarView::AI {
                    self.show_chat_panel = !self.show_chat_panel;
                } else {
                    if self.active_sidebar == *view {
                        self.show_sidebar = !self.show_sidebar;
                    } else {
                        self.active_sidebar = *view;
                        self.show_sidebar = true;
                    }
                }
            }
        }
    }

    // ==================================================================
    // Layout: Sidebar Content
    // ==================================================================

    fn render_sidebar(&mut self, ui: &mut egui::Ui) {
        match self.active_sidebar {
            SidebarView::Explorer => self.render_explorer_sidebar(ui),
            SidebarView::Search => self.render_search_sidebar(ui),
            SidebarView::SourceControl => self.render_source_control_sidebar(ui),
            SidebarView::Debug => self.render_debug_sidebar(ui),
            SidebarView::Extensions => self.render_extensions_sidebar(ui),
            SidebarView::AI => {
                // If AI is active in sidebar, show a compact AI panel
                self.render_compact_ai_sidebar(ui);
            }
        }
    }

    fn render_sidebar_header(&mut self, ui: &mut egui::Ui, title: &str) {
        // Sidebar header (like VS Code's "EXPLORER" section)
        let header_rect = ui.max_rect();
        ui.painter().rect_filled(
            Rect::from_min_max(
                egui::pos2(header_rect.left(), header_rect.top()),
                egui::pos2(header_rect.right(), header_rect.top() + 32.0),
            ),
            Rounding::default(),
            Color32::from_rgba_premultiplied(0, 0, 0, 20),
        );

        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(title)
                    .color(theme::TEXT_MUTED)
                    .size(11.0)
                    .strong(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);

                // Collapse button
                let collapse = ui.add(
                    egui::Button::new(
                        egui::RichText::new("−")
                            .color(theme::TEXT_MUTED)
                            .size(14.0),
                    )
                    .frame(false),
                );
                if collapse.clicked() {
                    self.show_sidebar = false;
                }
            });
        });

        // Separator
        ui.painter().line_segment(
            [
                egui::pos2(ui.max_rect().left(), 32.0),
                egui::pos2(ui.max_rect().right(), 32.0),
            ],
            Stroke::new(1.0, theme::BORDER_SECTION),
        );

        ui.add_space(4.0);
    }

    // ==================================================================
    // Explorer Sidebar (File Tree)
    // ==================================================================

    fn render_explorer_sidebar(&mut self, ui: &mut egui::Ui) {
        self.render_sidebar_header(ui, "EXPLORER");

        if self.file_tree.is_empty() {
            ui.add_space(16.0);
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("No folder open")
                        .color(theme::TEXT_MUTED)
                        .size(12.0),
                );
                ui.label(
                    egui::RichText::new("Open a folder to explore files")
                        .color(theme::TEXT_SECONDARY)
                        .size(11.0),
                );
            });
            return;
        }

        let mut clicked_path = None;
        let mut toggle_expanded = None;

        let available_width = ui.available_width();

        for (i, entry) in self.file_tree.iter().enumerate() {
            let indent = 8.0 + entry.depth as f32 * 16.0;
            let row_height = 22.0;
            let row_rect = egui::Rect::from_min_size(
                egui::pos2(ui.max_rect().left(), ui.cursor().top()),
                Vec2::new(available_width, row_height),
            );

            let response = ui
                .allocate_ui(Vec2::new(available_width, row_height), |ui| {
                    let painter = ui.painter();

                    // Indentation guide lines (VS Code tree lines)
                    if entry.depth > 0 {
                        for d in 1..=entry.depth {
                            let guide_x = 8.0 + (d as f32 - 1.0) * 16.0 + 8.0;
                            painter.line_segment(
                                [
                                    egui::pos2(guide_x, row_rect.top()),
                                    egui::pos2(guide_x, row_rect.bottom()),
                                ],
                                Stroke::new(1.0, theme::INDENT_GUIDE),
                            );
                        }
                    }

                    // Arrow or spacing for directory
                    if entry.is_dir {
                        let arrow = if entry.expanded { "▼" } else { "▶" };
                        ui.add_space(indent);
                        ui.label(
                            egui::RichText::new(arrow)
                                .color(theme::TEXT_MUTED)
                                .size(8.0),
                        );
                    } else {
                        ui.add_space(indent + 10.0);
                    }

                    // File icon
                    let icon = file_icon(&entry.name);
                    let icon_color = file_icon_color(&entry.name);
                    ui.label(egui::RichText::new(icon).color(icon_color).size(12.0));
                    ui.add_space(4.0);

                    // File name
                    let name_color = if entry.name == "Cargo.toml" || entry.name == "package.json" {
                        theme::ACCENT_YELLOW
                    } else {
                        theme::TEXT_PRIMARY
                    };
                    let label = ui.selectable_label(
                        false,
                        egui::RichText::new(&entry.name)
                            .color(name_color)
                            .size(12.0),
                    );

                    if label.clicked() {
                        if entry.is_dir {
                            toggle_expanded = Some(i);
                        } else {
                            clicked_path = Some(entry.path.clone());
                        }
                    }
                    if label.secondary_clicked() && entry.is_dir {
                        toggle_expanded = Some(i);
                    }
                })
                .response;

            // Active file highlight (VS Code style)
            let mut is_active_file = false;
            if let Some(open_file) = self.open_files.get(self.active_tab) {
                if entry.path == open_file.path {
                    is_active_file = true;
                    ui.painter()
                        .rect_filled(row_rect, Rounding::default(), theme::TREE_SELECTED_BG);
                    // Left accent bar
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            egui::pos2(row_rect.left(), row_rect.top()),
                            egui::pos2(row_rect.left() + 2.0, row_rect.bottom()),
                        ),
                        Rounding::default(),
                        theme::ACCENT_BLUE,
                    );
                }
            }

            // Hover highlight (subtle, only if not active)
            if response.hovered() && !is_active_file {
                ui.painter().rect_filled(row_rect, Rounding::default(), theme::TREE_HOVER_BG);
            }
        }

        if let Some(idx) = toggle_expanded {
            let was_expanded = self.file_tree[idx].expanded;
            self.file_tree[idx].expanded = !was_expanded;
            let path = self.file_tree[idx].path.clone();
            let depth = self.file_tree[idx].depth + 1;

            if !was_expanded {
                if let Ok(entries) = std::fs::read_dir(&path) {
                    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                    sorted.sort_by(|a, b| {
                        let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        b_dir
                            .cmp(&a_dir)
                            .then_with(|| a.file_name().cmp(&b.file_name()))
                    });

                    let mut new_entries = Vec::new();
                    for entry in sorted {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with('.') || name == "node_modules" || name == "target" {
                            continue;
                        }
                        new_entries.push(FileEntry {
                            name,
                            path: entry.path(),
                            is_dir: entry.file_type().map(|t| t.is_dir()).unwrap_or(false),
                            depth,
                            expanded: false,
                        });
                    }
                    self.file_tree.splice((idx + 1)..(idx + 1), new_entries);
                }
            } else {
                let parent_depth = self.file_tree[idx].depth;
                let mut remove_end = idx + 1;
                while remove_end < self.file_tree.len()
                    && self.file_tree[remove_end].depth > parent_depth
                {
                    remove_end += 1;
                }
                self.file_tree.drain((idx + 1)..remove_end);
            }
        }

        if let Some(path) = clicked_path {
            self.open_file(&path);
        }
    }

    // ==================================================================
    // Placeholder Sidebars
    // ==================================================================

    fn render_search_sidebar(&mut self, ui: &mut egui::Ui) {
        self.render_sidebar_header(ui, "SEARCH");

        ui.add_space(40.0);
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("🔍 Search")
                    .color(theme::TEXT_MUTED)
                    .size(24.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Coming soon")
                    .color(theme::TEXT_SECONDARY)
                    .size(12.0),
            );
        });
    }

    fn render_source_control_sidebar(&mut self, ui: &mut egui::Ui) {
        self.render_sidebar_header(ui, "SOURCE CONTROL");

        ui.add_space(40.0);
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("⎇ Git")
                    .color(theme::TEXT_MUTED)
                    .size(24.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Coming soon")
                    .color(theme::TEXT_SECONDARY)
                    .size(12.0),
            );
        });
    }

    fn render_debug_sidebar(&mut self, ui: &mut egui::Ui) {
        self.render_sidebar_header(ui, "RUN AND DEBUG");

        ui.add_space(40.0);
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("▶ Debug")
                    .color(theme::TEXT_MUTED)
                    .size(24.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Coming soon")
                    .color(theme::TEXT_SECONDARY)
                    .size(12.0),
            );
        });
    }

    fn render_extensions_sidebar(&mut self, ui: &mut egui::Ui) {
        self.render_sidebar_header(ui, "EXTENSIONS");

        ui.add_space(40.0);
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("◆ Extensions")
                    .color(theme::TEXT_MUTED)
                    .size(24.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Coming soon")
                    .color(theme::TEXT_SECONDARY)
                    .size(12.0),
            );
        });
    }

    fn render_compact_ai_sidebar(&mut self, ui: &mut egui::Ui) {
        self.render_sidebar_header(ui, "AI CHAT");
        // The full chat panel is on the right; this is a compact version
        ui.label(
            egui::RichText::new("✦ AI Panel")
                .color(theme::TEXT_MUTED)
                .size(12.0),
        );
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Use the right-side chat panel\nfor AI interactions.")
                .color(theme::TEXT_SECONDARY)
                .size(11.0),
        );
    }

    // ==================================================================
    // Layout: Status Bar (VS Code style)
    // ==================================================================

    fn render_status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .frame(theme::status_bar_frame())
            .min_height(22.0)
            .show(ctx, |ui| {
                let height = ui.available_height();
                let left_bg = theme::STATUS_BAR_BG;
                let right_bg = Color32::from_rgba_premultiplied(
                    theme::ACCENT_BLUE.r(),
                    theme::ACCENT_BLUE.g(),
                    theme::ACCENT_BLUE.b(),
                    40,
                );

                ui.horizontal(|ui| {
                    // Left section
                    let left_rect = Rect::from_min_size(
                        ui.max_rect().left_top(),
                        Vec2::new(ui.available_width() * 0.5, height),
                    );
                    ui.painter().rect_filled(left_rect, Rounding::default(), left_bg);

                    ui.add_space(6.0);

                    // Git branch indicator (placeholder)
                    ui.label(
                        egui::RichText::new("⎇ main")
                            .color(theme::TEXT_SECONDARY)
                            .size(11.0),
                    );
                    ui.add_space(6.0);

                    // Modified indicator
                    if let Some(file) = self.open_files.get(self.active_tab) {
                        if file.modified {
                            ui.label(
                                egui::RichText::new("● Modified")
                                    .color(theme::STATUS_MODIFIED)
                                    .size(11.0),
                            );
                            ui.add_space(6.0);
                        }
                    }

                    // File name
                    if let Some(file) = self.open_files.get(self.active_tab) {
                        ui.label(
                            egui::RichText::new(&file.name)
                                .color(theme::TEXT_PRIMARY)
                                .size(11.0),
                        );
                        ui.add_space(4.0);
                        // Language
                        if let Some(ref lang) = self.editor.language_id {
                            ui.label(
                                egui::RichText::new(lang)
                                    .color(theme::TEXT_MUTED)
                                    .size(11.0),
                            );
                        }
                    }

                    // Right section
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let right_rect = Rect::from_min_size(
                            egui::pos2(ui.max_rect().left(), ui.max_rect().top()),
                            Vec2::new(ui.available_width(), height),
                        );
                        ui.painter().rect_filled(right_rect, Rounding::default(), right_bg);

                        // AI status
                        let (ai_label, ai_color) = if self.waiting_for_response {
                            ("⟳ Thinking...", theme::STATUS_AI_THINKING)
                        } else {
                            (&self.ai_status as &str, theme::STATUS_AI_READY)
                        };
                        ui.label(egui::RichText::new(ai_label).color(ai_color).size(11.0));
                        ui.add_space(8.0);

                        // Encoding
                        ui.label(
                            egui::RichText::new("UTF-8")
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                        ui.add_space(8.0);

                        // Indentation
                        ui.label(
                            egui::RichText::new("Spaces: 4")
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                        ui.add_space(8.0);

                        // Line/Column
                        let pos = self.editor.cursors.primary().position;
                        let (line, col) =
                            self.editor.buffer.byte_to_line_col(pos).unwrap_or((0, 0));
                        ui.label(
                            egui::RichText::new(format!("Ln {}, Col {}", line + 1, col + 1))
                                .color(theme::TEXT_SECONDARY)
                                .size(11.0),
                        );
                        ui.add_space(8.0);

                        // Status text
                        ui.label(
                            egui::RichText::new(&self.status_text)
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                    });
                });
            });
    }

    // ==================================================================
    // Layout: Editor Area (tabs + content)
    // ==================================================================

    fn render_editor_area(&mut self, ui: &mut egui::Ui) {
        if self.open_files.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.label(
                        egui::RichText::new("✦ Aurora")
                            .color(theme::ACCENT_BLUE)
                            .size(36.0)
                            .strong(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Open a file or folder to start editing")
                            .color(theme::TEXT_MUTED)
                            .size(14.0),
                    );
                    ui.add_space(20.0);

                    // Quick actions
                    let open_btn = egui::Button::new(
                        egui::RichText::new("  📁  Open File  ")
                            .color(theme::ACCENT_BLUE)
                            .size(13.0),
                    )
                    .fill(theme::BG_ACTIVE)
                    .rounding(Rounding::same(4.0));
                    if ui.add(open_btn).clicked() {
                        // Would trigger file dialog
                    }
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new("Ctrl+O  —  Open File  ·  Ctrl+Shift+O  —  Open Folder")
                            .color(theme::TEXT_SECONDARY)
                            .size(12.0),
                    );
                });
            });
            return;
        }

        // Tab bar
        let tab_bar_height = 35.0;
        let avail = ui.available_rect_before_wrap();
        let tab_bar_rect = Rect::from_min_max(
            egui::pos2(avail.left(), avail.top()),
            egui::pos2(avail.right(), avail.top() + tab_bar_height),
        );

        // Tab bar background
        ui.painter()
            .rect_filled(tab_bar_rect, Rounding::default(), theme::TAB_BAR_BG);

        // Top border of tab bar
        ui.painter().line_segment(
            [
                egui::pos2(tab_bar_rect.left(), tab_bar_rect.top()),
                egui::pos2(tab_bar_rect.right(), tab_bar_rect.top()),
            ],
            Stroke::new(1.0, theme::BORDER_SECTION),
        );

        // Render each tab
        let mut close_tab_idx = None;
        let mut switch_to_tab = None;

        // Calculate tab widths
        let tab_count = self.open_files.len().max(1);
        let tab_max_width = 160.0;
        let tab_min_width = 100.0;
        let tab_width = ((tab_bar_rect.width() - 4.0) / tab_count as f32)
            .min(tab_max_width)
            .max(tab_min_width);

        for (i, file) in self.open_files.iter().enumerate() {
            let x = tab_bar_rect.left() + 2.0 + i as f32 * tab_width;
            let is_active = i == self.active_tab;

            let tab_rect = Rect::from_min_max(
                egui::pos2(x, tab_bar_rect.top() + 1.0),
                egui::pos2(
                    (x + tab_width - 2.0).min(tab_bar_rect.right()),
                    tab_bar_rect.bottom(),
                ),
            );

            // Tab background
            if is_active {
                // Active tab: blend with editor background
                ui.painter().rect_filled(
                    tab_rect,
                    Rounding::same(4.0),
                    theme::TAB_ACTIVE_BG,
                );
                // Bottom border (active tab extends into editor)
                ui.painter().rect_filled(
                    Rect::from_min_max(
                        egui::pos2(tab_rect.left(), tab_rect.bottom() - 1.0),
                        egui::pos2(tab_rect.right(), tab_rect.bottom()),
                    ),
                    Rounding::default(),
                    theme::TAB_ACTIVE_BG,
                );
                // Top accent line
                ui.painter().rect_filled(
                    Rect::from_min_max(
                        egui::pos2(tab_rect.left() + 4.0, tab_rect.top()),
                        egui::pos2(tab_rect.right() - 4.0, tab_rect.top() + 2.0),
                    ),
                    Rounding::same(1.0),
                    theme::ACCENT_BLUE,
                );
            } else {
                // Inactive tab
                ui.painter().rect_filled(
                    tab_rect,
                    Rounding::same(4.0),
                    theme::TAB_INACTIVE_BG,
                );
                // Subtle right border between tabs
                ui.painter().line_segment(
                    [
                        egui::pos2(tab_rect.right(), tab_rect.top() + 6.0),
                        egui::pos2(tab_rect.right(), tab_rect.bottom() - 4.0),
                    ],
                    Stroke::new(1.0, theme::BORDER_SECTION),
                );
            }

            // Hover effect for inactive tabs
            if !is_active {
                let tab_interact = ui.interact(
                    tab_rect,
                    egui::Id::new(("tab", i)),
                    egui::Sense::click(),
                );
                if tab_interact.hovered() {
                    ui.painter().rect_filled(
                        tab_rect.shrink2(Vec2::new(2.0, 2.0)),
                        Rounding::same(4.0),
                        Color32::from_rgba_premultiplied(255, 255, 255, 6),
                    );
                }
            }

            // File icon + name
            let icon = file_icon(&file.name);
            let label = if file.modified {
                format!("● {}", file.name)
            } else {
                file.name.clone()
            };
            let label_color = if is_active {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            };

            ui.painter().text(
                egui::pos2(tab_rect.left() + 10.0, tab_rect.center().y),
                egui::Align2::LEFT_CENTER,
                &format!("{}  {}", icon, label),
                egui::FontId::new(12.0, egui::FontFamily::Proportional),
                label_color,
            );

            // Close button
            let close_btn_rect = Rect::from_center_size(
                egui::pos2(tab_rect.right() - 12.0, tab_rect.center().y),
                Vec2::splat(16.0),
            );

            let close_interact = ui.interact(
                close_btn_rect,
                egui::Id::new(("tab_close", i)),
                egui::Sense::click(),
            );
            if close_interact.hovered() || close_interact.is_pointer_button_down_on() {
                ui.painter()
                    .rect_filled(close_btn_rect, Rounding::same(3.0), theme::BG_MUTED);
            }
            ui.painter().text(
                close_btn_rect.center(),
                egui::Align2::CENTER_CENTER,
                "×",
                egui::FontId::new(12.0, egui::FontFamily::Proportional),
                if close_interact.hovered() {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_MUTED
                },
            );

            // Tab click
            if is_active {
                // For active tab, only close button click works
                if close_interact.clicked() {
                    close_tab_idx = Some(i);
                }
            } else {
                let tab_interact = ui.interact(
                    tab_rect,
                    egui::Id::new(("tab_click", i)),
                    egui::Sense::click(),
                );
                if tab_interact.clicked()
                    && !close_btn_rect.contains(
                        ui.input(|i| i.pointer.interact_pos().unwrap_or_default()),
                    )
                {
                    switch_to_tab = Some(i);
                }
                if close_interact.clicked() {
                    close_tab_idx = Some(i);
                }
            }
            // Middle-click to close
            if tab_interact_middle_click(ui, tab_rect, i) {
                close_tab_idx = Some(i);
            }
        }

        if let Some(idx) = close_tab_idx {
            self.close_tab(idx);
        }
        if let Some(idx) = switch_to_tab {
            self.active_tab = idx;
            if let Some(file) = self.open_files.get(idx) {
                self.editor.load_text(&file.content);
            }
        }

        // Bottom separator line for tab bar
        ui.painter().line_segment(
            [
                egui::pos2(tab_bar_rect.left(), tab_bar_rect.bottom()),
                egui::pos2(tab_bar_rect.right(), tab_bar_rect.bottom()),
            ],
            Stroke::new(1.0, theme::BORDER_SECTION),
        );

        // Editor content area
        let editor_rect = Rect::from_min_max(
            egui::pos2(avail.left(), tab_bar_rect.bottom()),
            egui::pos2(avail.right(), avail.bottom()),
        );

        if editor_rect.height() <= 0.0 {
            return;
        }

        let line_height = 19.0;
        let visible_lines = (editor_rect.height() / line_height).floor() as usize;
        self.editor.viewport.resize(visible_lines.max(5));

        let mut editor_ui = ui.child_ui(editor_rect, *ui.layout());

        egui::ScrollArea::vertical()
            .id_source("editor_scroll")
            .show(&mut editor_ui, |ui| {
                ui.set_min_width(editor_rect.width());

                let line_count = self.editor.buffer.len_lines();
                let (start_line, end_line) = self.editor.viewport.render_range();
                let gutter_width = 52.0;

                let cursor_pos = self.editor.cursors.primary().position;
                let (cursor_line, _) = self
                    .editor
                    .buffer
                    .byte_to_line_col(cursor_pos)
                    .unwrap_or((0, 0));

                for line_idx in start_line..end_line.min(line_count) {
                    let row_height = line_height;
                    let row_top = ui.cursor().top();
                    let row_rect = Rect::from_min_size(
                        egui::pos2(editor_rect.left(), row_top),
                        Vec2::new(editor_rect.width(), row_height),
                    );

                    // Current line highlight
                    if line_idx == cursor_line {
                        ui.painter().rect_filled(
                            row_rect,
                            Rounding::default(),
                            theme::CURSOR_LINE_BG,
                        );
                    }

                    // Line number gutter
                    let gutter_rect = Rect::from_min_size(
                        egui::pos2(row_rect.left(), row_rect.top()),
                        Vec2::new(gutter_width, row_height),
                    );
                    // Gutter background
                    let gutter_bg = if line_idx == cursor_line {
                        theme::GUTTER_ACTIVE_LINE_BG
                    } else {
                        theme::GUTTER_BG
                    };
                    ui.painter()
                        .rect_filled(gutter_rect, Rounding::default(), gutter_bg);

                    // Gutter right border
                    ui.painter().line_segment(
                        [
                            egui::pos2(gutter_rect.right(), gutter_rect.top()),
                            egui::pos2(gutter_rect.right(), gutter_rect.bottom()),
                        ],
                        Stroke::new(1.0, theme::BORDER_SECTION),
                    );

                    // Line number
                    let line_num_color = if line_idx == cursor_line {
                        theme::GUTTER_ACTIVE_LINE
                    } else {
                        theme::GUTTER_LINE_NUM
                    };

                    let line_num = format!("{}", line_idx + 1);
                    ui.painter().text(
                        egui::pos2(gutter_rect.right() - 8.0, row_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        &line_num,
                        egui::FontId::new(11.0, egui::FontFamily::Monospace),
                        line_num_color,
                    );

                    // Code
                    if let Ok(line) = self.editor.buffer.get_line(line_idx) {
                        let line_text = line.trim_end_matches('\n');
                        let code_x = gutter_rect.right() + 8.0;

                        self.render_highlighted_line(
                            ui,
                            line_text,
                            line_idx,
                            code_x,
                            row_rect.center().y,
                        );
                    }

                    // Cursor bar
                    if line_idx == cursor_line {
                        let cursor_col = self
                            .editor
                            .buffer
                            .byte_to_line_col(cursor_pos)
                            .unwrap_or((0, 0))
                            .1;
                        let cursor_x = gutter_rect.right() + 8.0 + cursor_col as f32 * 7.6;
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                egui::pos2(cursor_x, row_rect.top()),
                                Vec2::new(2.0, row_height),
                            ),
                            Rounding::default(),
                            theme::CURSOR_COLOR,
                        );
                    }

                    ui.allocate_space(Vec2::new(editor_rect.width(), row_height));
                }
            });
    }

    fn render_highlighted_line(
        &self,
        ui: &mut egui::Ui,
        line_text: &str,
        line_idx: usize,
        code_x: f32,
        center_y: f32,
    ) {
        let line_start_byte = self
            .editor
            .buffer
            .line_col_to_byte(line_idx, 0)
            .unwrap_or(0);
        let highlights = &self.editor.highlights.ranges;

        let line_highlights: Vec<_> = highlights
            .iter()
            .filter(|r| r.start < line_start_byte + line_text.len() && r.end > line_start_byte)
            .collect();

        if line_highlights.is_empty() {
            ui.painter().text(
                egui::pos2(code_x, center_y),
                egui::Align2::LEFT_CENTER,
                line_text,
                egui::FontId::new(13.0, egui::FontFamily::Monospace),
                theme::TEXT_PRIMARY,
            );
            return;
        }

        // Build a LayoutJob for syntax-colored text
        let mut job = egui::text::LayoutJob::default();
        let mut cursor_byte = 0usize;

        for range in &line_highlights {
            let rel_start = range.start.saturating_sub(line_start_byte);
            let rel_end = (range.end - line_start_byte).min(line_text.len());

            // Text before
            if cursor_byte < rel_start {
                let before = &line_text[cursor_byte..rel_start];
                if !before.is_empty() {
                    job.append(
                        before,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::new(13.0, egui::FontFamily::Monospace),
                            color: theme::TEXT_PRIMARY,
                            ..Default::default()
                        },
                    );
                }
            }

            // Highlighted
            if rel_start < line_text.len() && rel_start < rel_end {
                let highlighted = &line_text[rel_start..rel_end];
                if !highlighted.is_empty() {
                    let color = theme::scope_to_color(&range.scope);
                    job.append(
                        highlighted,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::new(13.0, egui::FontFamily::Monospace),
                            color,
                            ..Default::default()
                        },
                    );
                }
            }

            cursor_byte = rel_end.max(cursor_byte);
        }

        // Remaining
        if cursor_byte < line_text.len() {
            let remaining = &line_text[cursor_byte..];
            if !remaining.is_empty() {
                job.append(
                    remaining,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::new(13.0, egui::FontFamily::Monospace),
                        color: theme::TEXT_PRIMARY,
                        ..Default::default()
                    },
                );
            }
        }

        let galley = ui.ctx().fonts(|f| f.layout_job(job));
        ui.painter()
            .galley(egui::pos2(code_x, center_y - galley.size().y / 2.0), galley);
    }

    // ==================================================================
    // Layout: Chat Panel
    // ==================================================================

    fn render_chat_panel(&mut self, ui: &mut egui::Ui) {
        // Header
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("✦ CHAT")
                    .color(theme::TEXT_MUTED)
                    .size(11.0)
                    .strong(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if ui.add(
                    egui::Button::new(
                        egui::RichText::new("×")
                            .color(theme::TEXT_MUTED)
                            .size(14.0),
                    )
                    .frame(false),
                ).on_hover_text("Close panel").clicked() {
                    self.show_chat_panel = false;
                }
                ui.add_space(4.0);
                if ui.add(
                    egui::Button::new(
                        egui::RichText::new("🗑")
                            .color(theme::TEXT_MUTED)
                            .size(12.0),
                    )
                    .frame(false),
                ).on_hover_text("Clear chat").clicked() {
                    self.chat_messages.clear();
                }
            });
        });

        // Separator
        ui.painter().line_segment(
            [
                egui::pos2(ui.max_rect().left(), ui.max_rect().top() + 30.0),
                egui::pos2(ui.max_rect().right(), ui.max_rect().top() + 30.0),
            ],
            Stroke::new(1.0, theme::BORDER_SECTION),
        );

        // Check for AI response
        if let Some(rx) = &self.ai_rx {
            if let Ok(response) = rx.try_recv() {
                self.chat_messages.push(ChatMessage {
                    role: MessageRole::Assistant,
                    content: response,
                    streaming: false,
                });
                self.waiting_for_response = false;
                self.ai_status = "AI Ready".into();
                self.status_text = "Ready".into();
                self.ai_rx = None;
            }
        }

        let avail = ui.available_rect_before_wrap();
        let input_area_height = 56.0;
        let messages_height = (avail.height() - input_area_height).max(50.0);
        let messages_rect = Rect::from_min_max(
            egui::pos2(avail.left(), avail.top()),
            egui::pos2(avail.right(), avail.top() + messages_height),
        );

        let mut messages_ui = ui.child_ui(messages_rect, *ui.layout());

        egui::ScrollArea::vertical()
            .id_source("chat_scroll")
            .auto_shrink([false, false])
            .show(&mut messages_ui, |ui| {
                ui.add_space(8.0);

                for msg in &self.chat_messages {
                    let (role_name, role_color, bubble_color) = match msg.role {
                        MessageRole::User => (
                            "You",
                            theme::ACCENT_BLUE,
                            Color32::from_rgba_premultiplied(
                                theme::ACCENT_BLUE.r(),
                                theme::ACCENT_BLUE.g(),
                                theme::ACCENT_BLUE.b(),
                                25,
                            ),
                        ),
                        MessageRole::Assistant => (
                            "Aurora",
                            theme::ACCENT_GREEN,
                            Color32::from_rgba_premultiplied(
                                theme::ACCENT_GREEN.r(),
                                theme::ACCENT_GREEN.g(),
                                theme::ACCENT_GREEN.b(),
                                20,
                            ),
                        ),
                        MessageRole::System => {
                            ("System", theme::TEXT_MUTED, theme::BG_ACTIVE)
                        }
                    };

                    // Message bubble
                    let msg_frame = Frame {
                        fill: bubble_color,
                        rounding: Rounding::same(8.0),
                        inner_margin: Margin::symmetric(10.0, 8.0),
                        ..Frame::none()
                    };

                    msg_frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Role badge
                            ui.label(
                                egui::RichText::new(role_name)
                                    .color(role_color)
                                    .size(11.0)
                                    .strong(),
                            );
                            if msg.streaming {
                                ui.label(
                                    egui::RichText::new("⟳")
                                        .color(theme::TEXT_MUTED)
                                        .size(10.0),
                                );
                            }
                        });
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&msg.content)
                                .color(theme::TEXT_PRIMARY)
                                .size(12.0),
                        );
                    });
                    ui.add_space(8.0);
                }

                // Thinking indicator
                if self.waiting_for_response {
                    let frame = Frame {
                        fill: Color32::from_rgba_premultiplied(
                            theme::ACCENT_GREEN.r(),
                            theme::ACCENT_GREEN.g(),
                            theme::ACCENT_GREEN.b(),
                            20,
                        ),
                        rounding: Rounding::same(8.0),
                        inner_margin: Margin::symmetric(10.0, 8.0),
                        ..Frame::none()
                    };
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Aurora")
                                    .color(theme::ACCENT_GREEN)
                                    .size(11.0)
                                    .strong(),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("⟳ Thinking...")
                                    .color(theme::TEXT_MUTED)
                                    .size(11.0),
                            );
                        });
                    });
                }
            });

        // Input area
        let input_rect = Rect::from_min_max(
            egui::pos2(avail.left(), avail.top() + messages_height + 4.0),
            egui::pos2(avail.right(), avail.bottom()),
        );

        let mut input_ui = ui.child_ui(input_rect, *ui.layout());

        // Input box with rounded corners
        Frame {
            fill: theme::BG_ACTIVE,
            rounding: Rounding::same(8.0),
            inner_margin: Margin::symmetric(10.0, 6.0),
            stroke: Stroke::new(1.0, theme::BORDER_SUBTLE),
            ..Frame::none()
        }
        .show(&mut input_ui, |ui| {
            ui.horizontal(|ui| {
                let input = ui.add_sized(
                    Vec2::new(ui.available_width() - 36.0, 36.0),
                    egui::TextEdit::multiline(&mut self.chat_input)
                        .desired_rows(1)
                        .hint_text("Ask Aurora something..."),
                );

                let send_clicked = ui.add(
                    egui::Button::new(
                        egui::RichText::new("→")
                            .color(if self.chat_input.trim().is_empty() {
                                theme::TEXT_MUTED
                            } else {
                                theme::ACCENT_BLUE
                            })
                            .size(18.0),
                    )
                    .frame(false),
                ).clicked();

                let enter_pressed = input.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));

                if (send_clicked || enter_pressed)
                    && !self.chat_input.trim().is_empty()
                    && !self.waiting_for_response
                {
                    let user_msg = self.chat_input.trim().to_string();
                    self.chat_input.clear();
                    self.send_chat_message(user_msg);
                }
            });
        });
    }

    fn render_agent_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("AGENT")
                    .color(theme::TEXT_MUTED)
                    .size(11.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if ui.add(
                    egui::Button::new(
                        egui::RichText::new("×")
                            .color(theme::TEXT_MUTED)
                            .size(14.0),
                    )
                    .frame(false),
                ).on_hover_text("Close panel").clicked() {
                    self.show_agent_panel = false;
                }
            });
        });

        // Separator
        ui.painter().line_segment(
            [
                egui::pos2(ui.max_rect().left(), 30.0),
                egui::pos2(ui.max_rect().right(), 30.0),
            ],
            Stroke::new(1.0, theme::BORDER_SECTION),
        );

        if self.agent_trace.is_empty() {
            ui.add_space(40.0);
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("No active agent task")
                        .color(theme::TEXT_MUTED)
                        .size(12.0),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Ask the AI to perform multi-step tasks\nand results will appear here.",
                    )
                    .color(theme::TEXT_SECONDARY)
                    .size(11.0),
                );
            });
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for step in &self.agent_trace {
                    let frame = Frame {
                        fill: Color32::from_rgba_premultiplied(
                            theme::ACCENT_PURPLE.r(),
                            theme::ACCENT_PURPLE.g(),
                            theme::ACCENT_PURPLE.b(),
                            15,
                        ),
                        rounding: Rounding::same(4.0),
                        inner_margin: Margin::symmetric(10.0, 6.0),
                        ..Frame::none()
                    };
                    frame.show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(step)
                                .color(theme::TEXT_PRIMARY)
                                .size(11.0),
                        );
                    });
                    ui.add_space(4.0);
                }
            });
    }
}

// ==================================================================
// eframe::App implementation
// ==================================================================

impl eframe::App for AuroraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Title bar at the top
        self.render_title_bar(ctx);

        // Activity bar (far left — VS Code style)
        egui::SidePanel::left("activity_bar")
            .resizable(false)
            .default_width(48.0)
            .frame(theme::activity_bar_frame())
            .show(ctx, |ui| {
                self.render_activity_bar(ui);
            });

        // Sidebar (left, between activity bar and editor)
        if self.show_sidebar {
            egui::SidePanel::left("sidebar")
                .resizable(true)
                .default_width(260.0)
                .min_width(180.0)
                .max_width(400.0)
                .frame(theme::sidebar_frame())
                .show(ctx, |ui| {
                    self.render_sidebar(ui);
                });
        }

        // Agent panel (right outer)
        if self.show_agent_panel && self.show_chat_panel {
            egui::SidePanel::right("agent_panel_outer")
                .resizable(true)
                .default_width(280.0)
                .min_width(200.0)
                .max_width(400.0)
                .frame(theme::panel_frame())
                .show(ctx, |ui| {
                    self.render_agent_panel(ui);
                });
        } else if self.show_agent_panel {
            egui::SidePanel::right("agent_panel")
                .resizable(true)
                .default_width(280.0)
                .min_width(200.0)
                .max_width(400.0)
                .frame(theme::panel_frame())
                .show(ctx, |ui| {
                    self.render_agent_panel(ui);
                });
        }

        // Chat panel (right inner)
        if self.show_chat_panel {
            egui::SidePanel::right("chat_panel")
                .resizable(true)
                .default_width(300.0)
                .min_width(220.0)
                .max_width(450.0)
                .frame(theme::panel_frame())
                .show(ctx, |ui| {
                    self.render_chat_panel(ui);
                });
        }

        // Central editor area
        egui::CentralPanel::default()
            .frame(theme::editor_frame())
            .show(ctx, |ui| {
                self.render_editor_area(ui);
            });

        // Status bar at the bottom
        self.render_status_bar(ctx);

        // Keep repainting while waiting for AI
        if self.waiting_for_response {
            ctx.request_repaint();
        }
    }
}

// ==================================================================
// Helper functions
// ==================================================================

/// Check for middle-click on a tab rect.
fn tab_interact_middle_click(ui: &egui::Ui, tab_rect: Rect, idx: usize) -> bool {
    let response = ui.interact(tab_rect, egui::Id::new(("tab_middle", idx)), egui::Sense::click());
    response.clicked_by(egui::PointerButton::Middle)
}


fn file_icon(filename: &str) -> &str {
    if filename.ends_with(".rs") {
        "R"
    } else if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        "T"
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        "J"
    } else if filename.ends_with(".py") {
        "P"
    } else if filename.ends_with(".go") {
        "G"
    } else if filename.ends_with(".json") {
        "{"
    } else if filename.ends_with(".yaml") || filename.ends_with(".yml") {
        "Y"
    } else if filename.ends_with(".md") {
        "M"
    } else if filename.ends_with(".html") {
        "H"
    } else if filename.ends_with(".css") || filename.ends_with(".scss") {
        "#"
    } else if filename == "Cargo.toml" || filename == "package.json" {
        "⚙"
    } else if filename.ends_with(".toml") {
        "T"
    } else if filename.ends_with(".gitignore") {
        "G"
    } else if filename.ends_with(".lock") {
        "🔒"
    } else if filename.ends_with(".env") {
        "E"
    } else {
        "·"
    }
}

fn file_icon_color(filename: &str) -> egui::Color32 {
    if filename.ends_with(".rs") {
        theme::ACCENT_BLUE
    } else if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        theme::ACCENT_BLUE
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        theme::ACCENT_YELLOW
    } else if filename.ends_with(".py") {
        theme::ACCENT_GREEN
    } else if filename.ends_with(".go") {
        theme::ACCENT_CYAN
    } else if filename.ends_with(".toml") || filename.ends_with(".json") {
        theme::ACCENT_YELLOW
    } else if filename.ends_with(".yaml") || filename.ends_with(".yml") {
        theme::ACCENT_RED
    } else if filename.ends_with(".md") {
        theme::ACCENT_BLUE
    } else if filename.ends_with(".html") {
        theme::ACCENT_ORANGE
    } else if filename.ends_with(".css") || filename.ends_with(".scss") {
        theme::ACCENT_PURPLE
    } else {
        theme::TEXT_SECONDARY
    }
}
