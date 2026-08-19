//! Serialization adapters shared by deterministic analysis reports.

/// Serializes an ordered map as a sequence of key-value pairs.
///
/// JSON requires object keys to be strings, while analysis evidence commonly
/// uses typed tuple or graph keys. The sequence representation remains
/// deterministic and rejects duplicate keys when decoded.
pub(crate) mod ordered_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(crate) fn serialize<K, V, S>(map: &BTreeMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        K: Ord + Serialize,
        V: Serialize,
        S: Serializer,
    {
        map.iter().collect::<Vec<_>>().serialize(serializer)
    }

    pub(crate) fn deserialize<'de, K, V, D>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        K: Ord + Deserialize<'de>,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let entries = Vec::<(K, V)>::deserialize(deserializer)?;
        let mut map = BTreeMap::new();
        for (key, value) in entries {
            if map.insert(key, value).is_some() {
                return Err(serde::de::Error::custom(
                    "duplicate key in ordered analysis evidence",
                ));
            }
        }
        Ok(map)
    }
}
