use anyhow::Result;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "IdeaIME";

pub fn is_enabled() -> Result<bool> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ)?;
    let value = run.get_value::<String, _>(VALUE_NAME).unwrap_or_default();
    Ok(!value.is_empty())
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(RUN_KEY)?;

    if enabled {
        let exe = std::env::current_exe()?;
        run.set_value(VALUE_NAME, &format!("\"{}\"", exe.display()))?;
    } else {
        let _ = run.delete_value(VALUE_NAME);
    }

    Ok(())
}
