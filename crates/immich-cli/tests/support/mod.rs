use std::process::Command;

pub fn python() -> Command {
    #[cfg(windows)]
    {
        let mut command = Command::new("py");
        command.arg("-3.12");
        command
    }
    #[cfg(not(windows))]
    {
        Command::new("python3")
    }
}
