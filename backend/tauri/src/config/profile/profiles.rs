use serde::Serialize;

#[derive(Serialize, specta::Type, Clone)]
pub struct Profiles {}

impl Profiles {
    pub fn new() -> Self {
        todo!()
    }
}
