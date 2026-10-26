use crate::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const FILTER: &str = "udp and (port 35000 or port 49152)";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CaptureBackend {
    MacTcpdump,
    LinuxTcpdump,
    WindowsDumpcap,
}

impl CaptureBackend {
    fn current() -> Result<Self> {
        match std::env::consts::OS {
            "macos" => Ok(Self::MacTcpdump),
            "linux" => Ok(Self::LinuxTcpdump),
            "windows" => Ok(Self::WindowsDumpcap),
            _ => Err("live capture is supported on macOS, Linux, and Windows".into()),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::MacTcpdump | Self::LinuxTcpdump => "tcpdump",
            Self::WindowsDumpcap => "dumpcap",
        }
    }

    fn program(self) -> Result<PathBuf> {
        match self {
            Self::MacTcpdump => {
                let path = PathBuf::from("/usr/sbin/tcpdump");
                if path.is_file() {
                    Ok(path)
                } else {
                    Err("macOS tcpdump unavailable at /usr/sbin/tcpdump".into())
                }
            },
            Self::LinuxTcpdump => ["/usr/sbin/tcpdump", "/usr/bin/tcpdump"]
                .into_iter()
                .map(PathBuf::from)
                .find(|path| path.is_file())
                .ok_or_else(|| "install tcpdump at /usr/sbin or /usr/bin".into()),
            Self::WindowsDumpcap => ["ProgramFiles", "ProgramFiles(x86)"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(|root| PathBuf::from(root).join("Wireshark").join("dumpcap.exe"))
                .find(|path| path.is_file())
                .ok_or_else(|| "install Wireshark dumpcap.exe in Program Files".into()),
        }
    }

    pub(crate) fn command(self, program: &Path, interface: &str) -> Result<Command> {
        validate_interface(interface)?;
        let mut command = Command::new(program);
        match self {
            Self::MacTcpdump | Self::LinuxTcpdump => {
                command.args(["-i", interface, "-s", "0", "-U", "-w", "-", FILTER]);
            },
            Self::WindowsDumpcap => {
                command.args([
                    "-i", interface, "-f", FILTER, "-F", "pcap", "-s", "0", "-q", "-w", "-",
                ]);
            },
        }
        Ok(command)
    }
}

pub(crate) fn host_command(interface: &str) -> Result<(Command, &'static str)> {
    validate_interface(interface)?;
    let backend = CaptureBackend::current()?;
    let program = backend.program()?;
    Ok((backend.command(&program, interface)?, backend.name()))
}

fn validate_interface(interface: &str) -> Result<()> {
    if interface.trim().is_empty()
        || interface.starts_with('-')
        || interface.len() > 256
        || interface.chars().any(char::is_control)
    {
        return Err("capture interface name invalid".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "../test_support/unit/capture_backend.rs"]
mod tests;
