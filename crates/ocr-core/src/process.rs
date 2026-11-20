//! Process helpers shared by the CLI and the GUI.

use std::process::Command;

/// Keep a child process from opening its own console window on Windows.
///
/// The release GUI is linked as a Windows-subsystem executable and has no
/// console. Without `CREATE_NO_WINDOW`, every console helper it starts
/// (`llama-server --version`, `taskkill`, ...) would flash a black window.
/// On other platforms this is a no-op.
pub fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}
