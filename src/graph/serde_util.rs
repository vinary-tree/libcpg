//! Serde utilities for Arc<str> and related types.
//!
//! This module provides custom serialization/deserialization for types
//! that don't have native serde support.

use std::sync::Arc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use smallvec::SmallVec;

/// Serialize Arc<str> as a string.
pub fn serialize_arc_str<S>(value: &Arc<str>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    value.as_ref().serialize(serializer)
}

/// Deserialize Arc<str> from a string.
pub fn deserialize_arc_str<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(Arc::from(s.as_str()))
}

/// Module for serializing Arc<str> fields.
pub mod arc_str {
    use super::*;

    pub fn serialize<S>(value: &Arc<str>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_arc_str(value, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_arc_str(deserializer)
    }
}

/// Module for serializing Option<Arc<str>> fields.
pub mod option_arc_str {
    use super::*;

    pub fn serialize<S>(value: &Option<Arc<str>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(s) => serializer.serialize_some(s.as_ref()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Arc<str>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        Ok(opt.map(|s| Arc::from(s.as_str())))
    }
}

/// Module for serializing SmallVec<[Arc<str>; 2]> fields specifically.
pub mod smallvec_arc_str_2 {
    use super::*;

    pub fn serialize<S>(
        value: &SmallVec<[Arc<str>; 2]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(value.len()))?;
        for item in value {
            seq.serialize_element(item.as_ref())?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<SmallVec<[Arc<str>; 2]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<String> = Vec::deserialize(deserializer)?;
        Ok(vec.into_iter().map(|s| Arc::from(s.as_str())).collect())
    }
}
