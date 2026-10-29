use std::{path::Path, process::Command};

use super::{Fixture, test_container};

impl Fixture {
    pub fn remote_path(&self, source: &Path) -> String {
        let source = drip_core::SourcePath::try_from_path(source).unwrap();
        format!(
            "/mirror/{}/{}/{}",
            self.pairing.mirror_directory(),
            source.drive(),
            source.components().join("/")
        )
    }

    pub fn server_exists(&self, source: &Path) -> bool {
        let container = test_container();
        let output = Command::new("wsl")
            .args([
                "-d",
                "Docker",
                "--",
                "docker",
                "exec",
                &container,
                "test",
                "-e",
                &self.remote_path(source),
            ])
            .output()
            .unwrap();
        assert!(
            matches!(output.status.code(), Some(0 | 1)),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.status.success()
    }
}
