//! UI support for Aurora.
//!
//! The GUI can be enabled with the `gui` feature. Without it, Aurora runs in
//! headless mode and prints status updates to the terminal.

mod status;
pub use status::SharedStatus;

#[cfg(feature = "gui")]
pub mod app;
#[cfg(feature = "gui")]
pub mod theme;
#[cfg(feature = "gui")]
mod window;

#[cfg(feature = "gui")]
pub use app::AuroraApp;

/// Start the UI and return a shared status handle.
pub fn start_ui() -> SharedStatus {
    let status = SharedStatus::new("mock_local");

    #[cfg(feature = "gui")]
    {
        println!("[ui] Launching GUI mode.");
        window::run();
    }

    #[cfg(not(feature = "gui"))]
    {
        println!(
            "[ui] Running in headless mode (build with `--features gui` to launch the window)."
        );
    }

    status
}
