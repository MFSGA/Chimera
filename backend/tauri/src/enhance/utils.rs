use crate::{
    config::profile::{item_type::ProfileUid, profiles::Profiles},
    enhance::chain::ChainItem,
};

pub fn convert_uids_to_scripts(profiles: &Profiles, uids: &[ProfileUid]) -> Vec<ChainItem> {
    uids.iter()
        .filter_map(|uid| profiles.get_item(uid).ok())
        .filter_map(<Option<ChainItem>>::from)
        .collect::<Vec<ChainItem>>()
}
