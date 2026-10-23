#[cfg(windows)]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/icons/favicon.ico");
    res.set("ProductName", "Blockidle");
    res.set("FileDescription", "Blockidle Desktop Client");
    res.set("LegalCopyright", "Copyright (c) Blockidle Contributors");
    if let Err(e) = res.compile() {
        eprintln!("Warning: Failed to compile windows resource: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
