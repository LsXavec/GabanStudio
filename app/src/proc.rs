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
