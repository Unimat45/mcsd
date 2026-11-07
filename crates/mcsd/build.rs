// The dashboard is embedded from web/build. Make sure the folder exists so the
// daemon compiles before the frontend has ever been built (it then serves a
// placeholder page).
fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/build");
    let _ = std::fs::create_dir_all(&dir);
    println!("cargo:rerun-if-changed=../../web/build");
}
