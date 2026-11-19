//! The desktop GUI of hone (concept Section 12, "Desktop GUI"). It reaches hone only through
//! the command line, in [`hone`].

mod app;
mod features;
pub mod hone;
mod ui;

/// Opens the window for the project in `root` and runs until it is closed.
pub fn launch(root: std::path::PathBuf) {
    app::launch(root);
}
