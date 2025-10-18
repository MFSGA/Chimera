use serde::Serialize;

use crate::config::profile::item_type::ProfileUid;

#[derive(Serialize, specta::Type, Clone)]
pub struct Profiles {
    pub current: Vec<ProfileUid>,
}

impl Profiles {
    pub fn new() -> Self {
        todo!()
    }
}
