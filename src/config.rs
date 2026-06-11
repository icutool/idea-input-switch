use anyhow::Result;
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

use crate::ime::InputMethod;

const APP_KEY: &str = "Software\\IdeaInputSwitch";
const INPUT_METHOD_VALUE: &str = "InputMethod";

pub fn load_input_method() -> InputMethod {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey(APP_KEY)
        .ok()
        .and_then(|key| key.get_value::<String, _>(INPUT_METHOD_VALUE).ok())
        .and_then(|value| InputMethod::from_config_value(&value))
        .unwrap_or_default()
}

pub fn save_input_method(input_method: InputMethod) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(APP_KEY)?;
    key.set_value(INPUT_METHOD_VALUE, &input_method.config_value())?;
    Ok(())
}
