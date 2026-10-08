use std::io;
use std::path::{Path, PathBuf};

/// `HERDR_SOCKET_PATH` (injected into plugin processes), else the default session's socket.
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("HERDR_SOCKET_PATH").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("herdr").join("herdr.sock");
        }
    }
    crate::paths::home_dir()
        .unwrap_or_default()
        .join(".config/herdr/herdr.sock")
}

#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;
#[cfg(windows)]
pub type Stream = std::fs::File;

#[cfg(unix)]
pub fn connect(path: &Path) -> io::Result<Stream> {
    Stream::connect(path)
}

/// herdr serves its API on the named pipe `\\.\pipe\` + the socket path; a pipe opens like a file.
#[cfg(windows)]
pub fn connect(path: &Path) -> io::Result<Stream> {
    const ERROR_PIPE_BUSY: i32 = 231;
    let name = pipe_name(path);
    for _ in 0..20 {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&name)
        {
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            other => return other,
        }
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "herdr pipe busy"))
}

#[cfg(windows)]
fn pipe_name(path: &Path) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\{}", path.display()))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn pipe_name_prefixes_socket_path() {
        assert_eq!(
            pipe_name(Path::new(r"C:\Users\me\AppData\Roaming\herdr\herdr.sock")),
            PathBuf::from(r"\\.\pipe\C:\Users\me\AppData\Roaming\herdr\herdr.sock")
        );
    }
}
