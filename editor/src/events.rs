/// Events emitted by the editor that external subsystems (LSP, AI) can subscribe to.
///
/// These are emitted by `Editor` methods and consumed by bridges/adapters.
#[derive(Debug, Clone)]
pub enum EditorEvent {
    /// A file was opened in the editor.
    FileOpened {
        uri: String,
        language_id: String,
        text: String,
    },
    /// The buffer content changed.
    BufferChanged {
        uri: String,
        version: i32,
        text: String,
    },
    /// A file was closed.
    FileClosed { uri: String },
    /// The cursor moved to a new position (line, column).
    CursorMoved { uri: String, line: u32, column: u32 },
    /// The editor gained focus on a file.
    FileFocused { uri: String },
}

/// A simple event collector that accumulates events during an edit session.
/// The consumer (e.g., LspBridge) drains events after each operation.
#[derive(Debug, Clone, Default)]
pub struct EventCollector {
    events: Vec<EditorEvent>,
}

impl EventCollector {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn push(&mut self, event: EditorEvent) {
        self.events.push(event);
    }

    pub fn drain(&mut self) -> Vec<EditorEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
