use std::{fs, path::Path};

use windows::Win32::System::Registry::REG_SZ;

use super::SettingsStore;
use crate::{
    settings::SettingsError,
    windows_integration::startup::{RUN_KEY, RUN_VALUE, RegistryValue, read_value, write_value},
};

impl SettingsStore {
    /// Registers or removes this executable from the current user's sign-in startup.
    pub fn set_start_at_sign_in(&self, enabled: bool) -> Result<(), SettingsError> {
        let executable = std::env::current_exe().map_err(SettingsError::Startup)?;
        set_start_at_sign_in_at(self, enabled, &executable, RUN_KEY)
    }
}

fn set_start_at_sign_in_at(
    store: &SettingsStore,
    enabled: bool,
    executable: &Path,
    run_key: &str,
) -> Result<(), SettingsError> {
    let _lock = store.lock()?;
    let mut settings = store.load_unlocked()?;
    let current = read_value(run_key, RUN_VALUE).map_err(SettingsError::Startup)?;
    let previous_owned = settings
        .startup_executable
        .as_deref()
        .and_then(|path| encode_command(path).ok());
    let owns_current = current.is_some() && current == previous_owned;
    if enabled && current.is_some() && !owns_current {
        return Err(SettingsError::StartupConflict);
    }

    let (startup_executable, next) = if enabled {
        let path = canonical_executable(executable)?;
        (Some(path.clone()), Some(command_value(&path)?))
    } else {
        (None, if owns_current { None } else { current.clone() })
    };
    if settings.start_at_sign_in == enabled
        && settings.startup_executable == startup_executable
        && current == next
    {
        return Ok(());
    }

    if next != current {
        write_value(run_key, RUN_VALUE, next.as_ref()).map_err(SettingsError::Startup)?;
    }
    settings.start_at_sign_in = enabled;
    settings.startup_executable = startup_executable;
    if let Err(operation) = store.save_unlocked(&settings) {
        if next != current
            && let Err(rollback) = write_value(run_key, RUN_VALUE, current.as_ref())
        {
            return Err(SettingsError::StartupRollback {
                operation: Box::new(operation),
                rollback,
            });
        }
        return Err(operation);
    }
    Ok(())
}

fn canonical_executable(executable: &Path) -> Result<String, SettingsError> {
    let path = fs::canonicalize(executable).map_err(SettingsError::Startup)?;
    if !path.is_file() {
        return Err(SettingsError::Startup(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "current executable is not a file",
        )));
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(SettingsError::StartupPath)
}

fn command_value(path: &str) -> Result<RegistryValue, SettingsError> {
    let value = encode_command(path)?;
    if value.data.len() / 2 > 261 {
        return Err(SettingsError::StartupPathTooLong);
    }
    Ok(value)
}

fn encode_command(path: &str) -> Result<RegistryValue, SettingsError> {
    if path.contains('"') || path.contains('\0') || !Path::new(path).is_absolute() {
        return Err(SettingsError::StartupPath);
    }
    let command: Vec<u16> = format!("\"{path}\" --background")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let data = command.into_iter().flat_map(u16::to_le_bytes).collect();
    Ok(RegistryValue { kind: REG_SZ, data })
}

#[cfg(test)]
mod tests;
