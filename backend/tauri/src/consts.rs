use once_cell::sync::Lazy;

#[cfg(target_os = "windows")]
pub static IS_PORTABLE: Lazy<bool> = Lazy::new(|| {
    if cfg!(windows) {
        let dir = crate::utils::dirs::app_install_dir().unwrap();
        let portable_file = dir.join(".config/PORTABLE");
        portable_file.exists()
    } else {
        false
    }
});
