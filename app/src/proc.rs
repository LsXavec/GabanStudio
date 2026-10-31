//! Child processes without console windows (GM1). A release build runs in
//! the GUI subsystem, so every console child it starts (ffmpeg, its
//! `-version` probe) would otherwise flash a terminal over the artist's work.
//! Every spawn in the app goes through [`command`]; devloop.rs keeps its own
//! two (the dev loop is a terminal workflow).

/// `std::process::Command::new(program)` with `CREATE_NO_WINDOW` on Windows.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (winbase.h): the child gets no console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// GM2: a folder in the system file browser (Settings ▸ General's "show
/// autosave folder") — a window the artist asked for, never a test's.
pub fn show_folder(dir: &std::path::Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = command(program).arg(dir).spawn();
}

/// AU9: `Open file`: a finished export in the program the system opens it
/// with. Reached only through `AppRequest::OpenFile`, on the artist's press.
pub fn open_file(file: &std::path::Path) {
    show_folder(file);
}

/// AU9: `Show in folder`: the folder, with the file picked out where the
/// system can do that. Reached only through `AppRequest::ShowInFolder`.
pub fn show_in_folder(file: &std::path::Path) {
    if cfg!(windows) {
        let mut select = std::ffi::OsString::from("/select,");
        select.push(file);
        let _ = command("explorer").arg(select).spawn();
    } else if let Some(dir) = file.parent() {
        show_folder(dir);
    }
}
