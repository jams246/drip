use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Absolute local Windows path represented without its root separator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct SourcePath {
    drive: char,
    components: Vec<String>,
}

impl SourcePath {
    /// Creates a path from a drive letter and original-case components.
    pub fn new(drive: char, components: Vec<String>) -> Result<Self, SourcePathError> {
        if !drive.is_ascii_alphabetic() {
            return Err(SourcePathError::InvalidDrive);
        }

        for component in &components {
            validate_component(component)?;
        }

        Ok(Self {
            drive: drive.to_ascii_uppercase(),
            components,
        })
    }

    /// Parses an absolute local drive-letter path using either Windows separator.
    pub fn parse_absolute(path: &str) -> Result<Self, SourcePathError> {
        let bytes = path.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || !matches!(bytes[2], b'\\' | b'/')
        {
            return Err(SourcePathError::NotAbsoluteLocalPath);
        }

        let remainder = &path[3..];
        if remainder.is_empty() {
            return Self::new(char::from(bytes[0]), Vec::new());
        }

        let mut components: Vec<String> = remainder.split(['\\', '/']).map(str::to_owned).collect();
        if components.last().is_some_and(String::is_empty) {
            components.pop();
        }

        Self::new(char::from(bytes[0]), components)
    }

    /// Converts a native path without lossy Unicode replacement.
    pub fn try_from_path(path: &Path) -> Result<Self, SourcePathError> {
        let path = path.to_str().ok_or(SourcePathError::NonUnicodePath)?;
        Self::parse_absolute(path)
    }

    /// Returns the uppercase drive letter.
    pub const fn drive(&self) -> char {
        self.drive
    }

    /// Returns path components with their original case preserved.
    pub fn components(&self) -> &[String] {
        &self.components
    }

    /// Returns whether this path has `prefix` under single-character Unicode uppercase matching.
    pub fn has_windows_prefix(&self, prefix: &Self) -> bool {
        self.drive == prefix.drive
            && self.components.len() >= prefix.components.len()
            && self
                .components
                .iter()
                .zip(&prefix.components)
                .all(|(component, prefix)| components_equal_ordinal_ignore_case(component, prefix))
    }

    /// Returns the canonical Windows path form with backslash separators.
    pub fn to_windows_path(&self) -> String {
        let mut path = String::new();
        path.push(self.drive);
        path.push(':');
        path.push('\\');
        path.push_str(&self.components.join("\\"));
        path
    }
}

fn components_equal_ordinal_ignore_case(left: &str, right: &str) -> bool {
    let mut left = left.chars();
    let mut right = right.chars();
    loop {
        match (left.next(), right.next()) {
            (Some(left), Some(right)) if left == right => {}
            (Some(left), Some(right)) => {
                let mut left = left.to_uppercase();
                let mut right = right.to_uppercase();
                if left.next() != right.next() || left.next().is_some() || right.next().is_some() {
                    return false;
                }
            }
            (None, None) => return true,
            _ => return false,
        }
    }
}

impl<'de> Deserialize<'de> for SourcePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WirePath {
            drive: char,
            components: Vec<String>,
        }

        let path = WirePath::deserialize(deserializer)?;
        Self::new(path.drive, path.components).map_err(serde::de::Error::custom)
    }
}

fn validate_component(component: &str) -> Result<(), SourcePathError> {
    if component.is_empty() {
        return Err(SourcePathError::EmptyComponent);
    }
    if matches!(component, "." | "..") {
        return Err(SourcePathError::DotComponent);
    }
    if component.contains(['\\', '/']) {
        return Err(SourcePathError::SeparatorInComponent);
    }
    if component.contains('\0') {
        return Err(SourcePathError::NullInComponent);
    }
    if component.contains(':') {
        return Err(SourcePathError::ColonInComponent);
    }
    Ok(())
}

/// Source-path validation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourcePathError {
    /// Drive letter was not an ASCII letter.
    #[error("drive letter must be an ASCII letter")]
    InvalidDrive,
    /// Input was not an absolute local drive-letter path.
    #[error("path must be an absolute local drive-letter path")]
    NotAbsoluteLocalPath,
    /// A path component was empty.
    #[error("path contains an empty component")]
    EmptyComponent,
    /// A path component was `.` or `..`.
    #[error("path contains a dot component")]
    DotComponent,
    /// A component contained a path separator.
    #[error("path component contains a separator")]
    SeparatorInComponent,
    /// A component contained NUL.
    #[error("path component contains NUL")]
    NullInComponent,
    /// A component contained a colon.
    #[error("path component contains a colon")]
    ColonInComponent,
    /// Native path could not be represented as Unicode.
    #[error("path cannot be represented losslessly as Unicode")]
    NonUnicodePath,
}
