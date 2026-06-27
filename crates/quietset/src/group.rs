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
