//! Version metadata for the setup exe (see app/build.rs — AV heuristics).

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "Gaban Studio");
        res.set("FileDescription", "Gaban Studio Setup");
        res.set("CompanyName", "Gaban Studio");
        res.set("LegalCopyright", "© 2026 Gaban Studio");
        res.set("OriginalFilename", "AnimStudio-Setup.exe");
        let _ = res.compile();
    }
}
