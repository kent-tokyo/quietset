use crate::observation::Observation;
use indexmap::IndexMap;

/// Groups observations by sample_id, preserving insertion order.
pub fn group_by_sample_id(
    observations: impl Iterator<Item = Observation>,
) -> IndexMap<String, Vec<Observation>> {
    let mut groups: IndexMap<String, Vec<Observation>> = IndexMap::new();
    for obs in observations {
        groups.entry(obs.sample_id.clone()).or_default().push(obs);
    }
    groups
}

/// Groups observations by `block_id`, preserving insertion order. Observations with no
/// `block_id` are skipped — they carry no block-level stability information.
pub fn group_by_block_id(
    observations: impl Iterator<Item = Observation>,
) -> IndexMap<String, Vec<Observation>> {
    let mut groups: IndexMap<String, Vec<Observation>> = IndexMap::new();
    for obs in observations {
        if let Some(block_id) = obs.block_id.clone() {
            groups.entry(block_id).or_default().push(obs);
        }
    }
    groups
}
