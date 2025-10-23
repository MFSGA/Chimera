use serde_yaml::Mapping;

#[derive(Default, Debug, Clone)]
pub struct IRuntime {
    pub config: Option<Mapping>,
}

impl IRuntime {
    pub fn new() -> Self {
        Self::default()
    }
}
