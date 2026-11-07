//! Serializing hash maps and sets in key order, for `#[serde(serialize_with)]`. A hash map's order
//! changes from run to run, so without this the same data would give different caches and data
//! packs. Reading them back needs nothing special.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    hash::BuildHasher,
};

use serde::{Serialize, Serializer};

pub fn map<K, V, H, S>(map: &HashMap<K, V, H>, serializer: S) -> Result<S::Ok, S::Error>
where
    K: Ord + Serialize,
    V: Serialize,
    H: BuildHasher,
    S: Serializer,
{
    serializer.collect_map(map.iter().collect::<BTreeMap<_, _>>())
}

pub fn set<T, H, S>(set: &HashSet<T, H>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Ord + Serialize,
    H: BuildHasher,
    S: Serializer,
{
    serializer.collect_seq(set.iter().collect::<BTreeSet<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Holder {
        #[serde(serialize_with = "map")]
        map: HashMap<i32, String>,
        #[serde(serialize_with = "set")]
        set: HashSet<i32>,
    }

    #[test]
    fn the_same_contents_give_the_same_bytes() {
        let bytes = |order: &[i32]| {
            let holder = Holder {
                map: order.iter().map(|&k| (k, k.to_string())).collect(),
                set: order.iter().copied().collect(),
            };
            postcard::to_stdvec(&holder).expect("serializes")
        };
        let forwards: Vec<i32> = (0..500).collect();
        let backwards: Vec<i32> = (0..500).rev().collect();
        assert_eq!(bytes(&forwards), bytes(&backwards));
    }
}
