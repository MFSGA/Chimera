use rfd::{MessageButtons, MessageDialog, MessageLevel};

pub fn error_dialog<T: Into<String>>(msg: T) {
    MessageDialog::new()
        .set_level(MessageLevel::Error)
        .set_title("Clash Chimera Error")
        .set_description(msg.into())
        .set_buttons(MessageButtons::Ok)
        .show();
}
