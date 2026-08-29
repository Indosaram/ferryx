pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[inline]
pub fn configure_no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[inline]
pub fn configure_tokio_no_window(
    cmd: &mut tokio::process::Command,
) -> &mut tokio::process::Command {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn no_window_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    configure_no_window(&mut c);
    c
}

pub fn no_window_tokio_command<S: AsRef<std::ffi::OsStr>>(
    program: S,
) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(program);
    configure_tokio_no_window(&mut c);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn test_no_window_command_spawn_succeeds() {
        // std exposes no getter for creation flags, so the assertion here is
        // behavioral: the helper-configured command must spawn and report
        // success. The flag application itself is a single cfg(windows) call
        // site in configure_no_window, verified by review.
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
        let status = no_window_command("cmd")
            .args(["/C", "exit", "0"])
            .status()
            .expect("cmd spawn with CREATE_NO_WINDOW must succeed");
        assert!(status.success());
    }

    #[cfg(not(windows))]
    #[test]
    fn test_configure_no_window_passthrough() {
        let mut cmd = std::process::Command::new("echo");
        let returned = configure_no_window(&mut cmd);
        assert_eq!(returned.get_program(), "echo");

        let mut tokio_cmd = tokio::process::Command::new("echo");
        let returned_tokio = configure_tokio_no_window(&mut tokio_cmd);
        assert_eq!(returned_tokio.as_std().get_program(), "echo");
    }
}
