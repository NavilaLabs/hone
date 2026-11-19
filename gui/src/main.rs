//! `hone-gui [project root]`: the desktop GUI of hone.

fn main() {
    let root = std::env::args_os()
        .nth(1)
        .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    hone_gui::launch(root);
}
