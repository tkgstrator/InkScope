use super::*;

fn args(backend: CaptureBackend, interface: &str) -> Vec<String> {
    backend
        .command(Path::new("capture-tool"), interface)
        .unwrap()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn capture_commands_write_classic_pcap_to_stdout() {
    for backend in [CaptureBackend::MacTcpdump, CaptureBackend::LinuxTcpdump] {
        assert_eq!(
            args(backend, "en18"),
            ["-i", "en18", "-s", "0", "-U", "-w", "-", FILTER]
        );
    }
    assert_eq!(
        args(CaptureBackend::WindowsDumpcap, "Ethernet 3"),
        [
            "-i",
            "Ethernet 3",
            "-f",
            FILTER,
            "-F",
            "pcap",
            "-s",
            "0",
            "-q",
            "-w",
            "-"
        ]
    );
}

#[test]
fn interface_names_allow_windows_devices_and_reject_options() {
    assert!(
        CaptureBackend::WindowsDumpcap
            .command(
                Path::new("dumpcap.exe"),
                r"\Device\NPF_{01234567-89AB-CDEF-0123-456789ABCDEF}"
            )
            .is_ok()
    );
    assert!(
        CaptureBackend::LinuxTcpdump
            .command(Path::new("tcpdump"), "eth0:1")
            .is_ok()
    );
    for invalid in ["", " ", "-w", "en0\nanother", "x\0y"] {
        assert!(
            CaptureBackend::MacTcpdump
                .command(Path::new("tcpdump"), invalid)
                .is_err()
        );
    }
}

#[test]
fn current_backend_matches_supported_host() {
    let backend = CaptureBackend::current();
    match std::env::consts::OS {
        "macos" => assert_eq!(backend.unwrap(), CaptureBackend::MacTcpdump),
        "linux" => assert_eq!(backend.unwrap(), CaptureBackend::LinuxTcpdump),
        "windows" => assert_eq!(backend.unwrap(), CaptureBackend::WindowsDumpcap),
        _ => assert!(backend.is_err()),
    }
}
