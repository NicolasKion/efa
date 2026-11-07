//! Build steps that depend on the target: the exe icon on Windows.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    windows_icon();
}

/// The exe's icon and version info. GPUI takes the window icon from resource 1, which is what
/// winresource writes. The resource compiler only exists on Windows hosts.
fn windows_icon() {
    let host = std::env::var("HOST").unwrap_or_default();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") || !host.contains("windows")
    {
        return;
    }
    println!("cargo:rerun-if-changed=assets/icons/efa.ico");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/icons/efa.ico")
        .set("ProductName", "efa")
        .set("FileDescription", "efa");
    resource
        .compile()
        .expect("embedding the Windows icon failed");
}
