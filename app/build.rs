//! Embed Windows version metadata (PSD-shipping amendment 2026-08-19: an
//! unsigned exe with NO version info scores higher on AV heuristics —
//! the owner's tester got flagged). Product identity is display-level
//! (Gaban Studio); file names stay plumbing-level (animstudio).

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "Gaban Studio");
        res.set("FileDescription", "Gaban Studio — 2D animation studio");
        res.set("CompanyName", "Gaban Studio");
        res.set("LegalCopyright", "© 2026 Gaban Studio");
        res.set("OriginalFilename", "animstudio.exe");
        let _ = res.compile();
    }
}
