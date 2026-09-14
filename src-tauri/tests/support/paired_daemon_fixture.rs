//! Test-owned filesystem and output boundaries. Never discovers a user's daemon.
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

pub struct PairedDaemonFixture {
    pub directory: TempDir,
    pub runtime: PathBuf,
    pub data: PathBuf,
    pub root: PathBuf,
    output: HashMap<(String, String), Vec<u8>>,
}

impl PairedDaemonFixture {
    pub fn new() -> std::io::Result<Self> {
        let directory = tempfile::Builder::new().prefix("fx-a01-").tempdir()?;
        let runtime = directory.path().join("run");
        let data = directory.path().join("data");
        let root = directory.path().join("root");
        for path in [&runtime, &data, &root] {
            std::fs::create_dir(path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        Ok(Self { directory, runtime, data, root, output: HashMap::new() })
    }

    fn key(host: &str, session: &str) -> (String, String) {
        (host.to_owned(), session.to_owned())
    }

    pub fn publish(&mut self, host: &str, session: &str, bytes: &[u8]) {
        self.output.entry(Self::key(host, session)).or_default().extend_from_slice(bytes);
    }

    pub fn output(&self, host: &str, session: &str) -> &[u8] {
        self.output.get(&Self::key(host, session)).map(Vec::as_slice).unwrap_or_default()
    }
}
