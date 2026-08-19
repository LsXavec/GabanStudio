//! AnimStudio-Setup (research/PSD-shipping.md, gate 2026-08-19): our own
//! installer — the release exe embedded at build time, seated into
//! %LOCALAPPDATA%\AnimStudio with a Start-Menu shortcut and a per-user
//! uninstall entry. Never touches HKLM or anything system-wide
//! (NEVER-DO 5). `--uninstall` removes exactly what install created.

use std::io::Write;
use std::path::PathBuf;

/// The app, baked in. Build order: `cargo build --release -p animstudio`
/// FIRST, then this crate.
static APP_EXE: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/release/animstudio.exe"));

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn install_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("AnimStudio"))
}

fn start_menu_lnk() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("Microsoft/Windows/Start Menu/Programs/AnimStudio.lnk"),
    )
}

fn pause_and_exit(code: i32) -> ! {
    print!("\npress Enter to close…");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    std::process::exit(code)
}

fn main() {
    let uninstall = std::env::args().any(|a| a == "--uninstall");
    if uninstall {
        run_uninstall();
    } else {
        run_install();
    }
}

fn run_install() {
    println!("AnimStudio {VERSION} — installing (per-user, no admin needed)");
    let Some(dir) = install_dir() else {
        println!("no LOCALAPPDATA on this machine — cannot install.");
        pause_and_exit(1);
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        println!("could not create {} — {e}", dir.display());
        pause_and_exit(1);
    }
    let exe = dir.join("animstudio.exe");
    // A running copy locks its exe: step it aside first (the same rename
    // dance the updater uses — reversible).
    if exe.exists() {
        let old = dir.join("animstudio-old.exe");
        let _ = std::fs::remove_file(&old);
        if let Err(e) = std::fs::rename(&exe, &old) {
            println!("an AnimStudio is running and could not step aside ({e}).");
            println!("close it and run this installer again.");
            pause_and_exit(1);
        }
    }
    if let Err(e) = std::fs::write(&exe, APP_EXE) {
        println!("could not write {} — {e}", exe.display());
        pause_and_exit(1);
    }
    println!("  seated {}", exe.display());

    // The uninstaller is this installer, kept beside the app.
    let me = std::env::current_exe().ok();
    let uninst = dir.join("uninstall.exe");
    if let Some(me) = &me
        && std::fs::copy(me, &uninst).is_ok()
    {
        println!("  seated {}", uninst.display());
    }

    // Start-Menu shortcut, written through the shell's own COM object.
    if let Some(lnk) = start_menu_lnk() {
        let script = format!(
            "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');\
             $s.TargetPath='{}';$s.WorkingDirectory='{}';\
             $s.Description='AnimStudio';$s.Save()",
            lnk.display(),
            exe.display(),
            dir.display()
        );
        let ok = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        println!(
            "  {} Start-Menu shortcut",
            if ok { "created" } else { "could not create" }
        );
    }

    // Per-user uninstall entry (HKCU only — NEVER-DO 5).
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AnimStudio";
    for (name, value) in [
        ("DisplayName", "AnimStudio".to_string()),
        ("DisplayVersion", VERSION.to_string()),
        ("InstallLocation", dir.display().to_string()),
        ("Publisher", "AnimStudio".to_string()),
        (
            "UninstallString",
            format!("\"{}\" --uninstall", uninst.display()),
        ),
    ] {
        let _ = std::process::Command::new("reg")
            .args(["add", key, "/v", name, "/d", &value, "/f"])
            .output();
    }
    println!("  registered the uninstall entry (per-user)");

    println!("\ninstalled. launching AnimStudio…");
    let _ = std::process::Command::new(&exe).current_dir(&dir).spawn();
    pause_and_exit(0);
}

fn run_uninstall() {
    println!("AnimStudio — uninstalling");
    let Some(dir) = install_dir() else {
        pause_and_exit(1);
    };
    let _ = std::fs::remove_file(dir.join("animstudio.exe"));
    let _ = std::fs::remove_file(dir.join("animstudio-old.exe"));
    let _ = std::fs::remove_file(dir.join("animstudio-new.exe"));
    if let Some(lnk) = start_menu_lnk() {
        let _ = std::fs::remove_file(lnk);
    }
    let _ = std::process::Command::new("reg")
        .args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AnimStudio",
            "/f",
        ])
        .output();
    // The app's own data (projects, brushes, config) is the OWNER'S:
    // deliberately left in place. Say so.
    println!("removed the app, shortcut and registry entry.");
    println!("your projects, brushes and settings in %APPDATA%/AnimStudio were kept.");
    // Self-delete last, via a detached cmd that waits for this process.
    if let Ok(me) = std::env::current_exe() {
        let _ = std::process::Command::new("cmd")
            .args([
                "/C",
                &format!("ping -n 3 127.0.0.1 >nul & del \"{}\"", me.display()),
            ])
            .spawn();
    }
    pause_and_exit(0);
}
