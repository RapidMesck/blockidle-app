#[cfg(windows)]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/icons/favicon.ico");
    res.set("ProductName", "Blockidle");
    res.set("FileDescription", "Blockidle Desktop Client");
    res.set("CompanyName", "rapidmesck");
    res.set("LegalCopyright", "Copyright (c) 2026 rapidmesck");
    if let Err(e) = res.compile() {
        eprintln!("Warning: Failed to compile windows resource: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
