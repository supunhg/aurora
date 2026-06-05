use ai::router::AIRequest;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--self-test") {
        run_self_test().await;
        return;
    }

    if args.iter().any(|a| a == "--editor-test") {
        run_editor_test();
        return;
    }

    if args.iter().any(|a| a == "--gui" || a == "--app") {
        // Launch the Tauri desktop application
        aurora_tauri::run();
        return;
    }

    // Default: show CLI help
    if let Some(path) = args.get(1) {
        println!("Aurora: opening {}", path);
    } else {
        println!("Aurora — AI-native code editor");
        println!();
        println!("Usage:");
        println!("  aurora --gui           Launch the desktop application");
        println!("  aurora --self-test     Run the AI router self-test");
        println!("  aurora --editor-test   Run the editor core test");
        println!();
        println!("The desktop app requires building the Tauri frontend first:");
        println!("  cd src-tauri && cargo build");
    }
}

fn run_editor_test() {
    use editor::Editor;
    use editor::RUST_KEYWORDS;

    println!("[editor-test] Starting Aurora editor core test...\n");

    // Test 1: Basic editing
    let mut ed = Editor::new();
    ed.insert_at_cursor("Hello, 世界!").unwrap();
    println!("Test 1 - Insert text:         \"{}\"", ed.buffer.text());
    assert_eq!(ed.buffer.text(), "Hello, 世界!");

    // Test 2: Cursor tracking
    println!(
        "Test 2 - Cursor position:     {}",
        ed.cursors.primary().position
    );
    // "Hello, 世界!" = 7 ASCII + 6 UTF-8 (世=3, 界=3) + 1 = 14 bytes
    assert_eq!(ed.cursors.primary().position, 14);

    // Test 3: Backspace (removes '!')
    ed.backspace().unwrap();
    println!("Test 3 - Backspace:           \"{}\"", ed.buffer.text());
    assert_eq!(ed.buffer.text(), "Hello, 世界");

    // Test 4: Undo/redo
    ed.undo().unwrap();
    println!("Test 4 - Undo:               \"{}\"", ed.buffer.text());
    assert_eq!(ed.buffer.text(), "Hello, 世界!");
    ed.redo().unwrap();
    println!("Test 4 - Redo:               \"{}\"", ed.buffer.text());
    assert_eq!(ed.buffer.text(), "Hello, 世界");

    // Test 5: Multi-line editing
    ed.load_text("line one\nline two\nline three");
    println!(
        "Test 5 - Load text:          {} lines",
        ed.buffer.len_lines()
    );
    assert_eq!(ed.buffer.len_lines(), 3);

    // Test 6: Cursor movement
    println!("Test 6 - Cursor movement:");
    ed.cursor_down().unwrap();
    let (line, col) = ed
        .buffer
        .byte_to_line_col(ed.cursors.primary().position)
        .unwrap();
    println!("       cursor_down -> line {}, col {}", line, col);
    assert_eq!(line, 1);
    ed.cursor_end().unwrap();
    let (_, col2) = ed
        .buffer
        .byte_to_line_col(ed.cursors.primary().position)
        .unwrap();
    println!("       cursor_end  -> col {} (before newline)", col2);
    assert_eq!(col2, 8); // "line two" = 8 chars (l-i-n-e-space-t-w-o)

    // Test 7: Home/End
    let mut ed2 = Editor::from_text("hello\nworld");
    ed2.cursor_end().unwrap();
    println!(
        "Test 7 - End of 'hello':     byte {}",
        ed2.cursors.primary().position
    );
    assert_eq!(ed2.cursors.primary().position, 5); // before newline
    ed2.cursor_home().unwrap();
    println!(
        "Test 7 - Home of 'hello':    byte {}",
        ed2.cursors.primary().position
    );
    assert_eq!(ed2.cursors.primary().position, 0);

    // Test 8: Syntax highlighting
    ed.load_text("fn main() {\n    let x = 42;\n}");
    ed.highlight_visible_range(RUST_KEYWORDS);
    println!(
        "Test 8 - Highlights:         {} ranges",
        ed.highlights.ranges.len()
    );
    assert!(!ed.highlights.ranges.is_empty());
    for r in &ed.highlights.ranges {
        println!("       [{:>4}..{:<4}] {}", r.start, r.end, r.scope);
    }

    // Test 9: Multi-cursor
    ed.cursors.add_cursor(0);
    ed.cursors.add_cursor(5);
    println!("Test 9 - Multi-cursor:       {} cursors", ed.cursors.len());
    assert_eq!(ed.cursors.len(), 3);

    // Test 10: Viewport
    let mut ed3 = Editor::with_viewport_height(
        "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10",
        5,
    );
    ed3.cursor_bottom().unwrap();
    let saved_line = ed3.viewport.first_line;
    ed3.viewport.scroll_up(1.0);
    println!(
        "Test 10 - Scrolled up from bottom: first_line={}",
        ed3.viewport.first_line
    );
    assert!(ed3.viewport.first_line < saved_line || ed3.viewport.first_line == 0);

    println!("\n[editor-test] All tests passed!");
}

async fn run_self_test() {
    println!("[self-test] Starting Aurora self-test...");

    plugin::init_plugin_host();

    // Create router
    let mut router = ai::AIRouter::new();

    // --- Native AI Providers ---
    println!("[self-test] Initializing native AI providers...");

    // Register Ollama (local inference, always available if running)
    let ollama = Arc::new(ai::OllamaProvider::new("llama3.2"));
    let ollama_available = ollama.is_available().await;
    if ollama_available {
        println!("[self-test] Ollama detected — registered as primary provider");
        router.register_provider(ollama);
    } else {
        println!("[self-test] Ollama not running at localhost:11434 — registering as fallback");
        // Register anyway — it will be skipped if unavailable during routing
        router.register_provider(ollama);
    }

    // Register Groq (cloud, requires GROQ_API_KEY env var)
    let groq_api_key = std::env::var("GROQ_API_KEY").unwrap_or_else(|_| {
        // Use a placeholder if no key is set — the mock fallback will handle it
        "placeholder-key".into()
    });
    if groq_api_key != "placeholder-key" {
        println!("[self-test] GROQ_API_KEY found — registering Groq provider");
    } else {
        println!("[self-test] No GROQ_API_KEY set — Groq will use placeholder key");
    }
    router.register_provider(Arc::new(ai::GroqProvider::new(&groq_api_key)));

    // Register OpenAI-compatible provider (if env var set)
    let openai_api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if !openai_api_key.is_empty() {
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        println!(
            "[self-test] OPENAI_API_KEY found — registering OpenAI provider ({})",
            model
        );
        router.register_provider(Arc::new(ai::OpenAIProvider::new(
            "openai",
            "https://api.openai.com",
            &openai_api_key,
            &model,
        )));
    }

    // Register MockCloud for testing fallback chain
    router.register_provider(Arc::new(ai::mock::MockCloudProvider::new(
        "mock_cloud/test",
        5,
    )));

    // Register LocalProvider as final fallback
    router.register_provider(Arc::new(ai::mock::LocalProvider::new()));

    println!(
        "[self-test] Registered {} providers",
        router.provider_count()
    );
    println!("[self-test] Routing 8 requests to test fallback chain...");

    // Make multiple requests to trigger rate limiting on the mock cloud provider
    for i in 1..=8 {
        let req = AIRequest {
            prompt: format!("Test request {}", i),
            conversation_id: None,
            model_hint: None,
            stream: false,
        };

        match router.route(req, None).await {
            Ok(meta) => {
                println!(
                    "[self-test] Request {}: routed to {}, fallbacks={}",
                    i, meta.routed_via, meta.fallback_attempts
                );
            }
            Err(e) => {
                eprintln!("[self-test] Request {} failed: {}", i, e);
            }
        }

        // Small delay between requests
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    println!("[self-test] Complete.");
}
