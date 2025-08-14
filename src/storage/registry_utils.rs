//! Registry Utilities: Shared functionality for all registry implementations
//!
//! This module provides common utilities used by both GraphRegistry and AgentRegistry,
//! avoiding code duplication while maintaining flexibility for each registry's specific needs.
//!
//! ## UUID Serialization
//!
//! The primary utilities are custom serde modules for UUID-based collections.
//! These handle the conversion between Uuid types and JSON string representations,
//! ensuring consistent serialization across all registries.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Custom serialization for HashMap with UUID keys
/// 
/// Serializes UUID keys as strings for JSON compatibility while maintaining
/// type safety in Rust code.
pub mod uuid_hashmap_serde {
    use super::*;
    
    pub fn serialize<S, V>(map: &HashMap<Uuid, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        V: Serialize,
    {
        let string_map: HashMap<String, &V> = map
            .iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        string_map.serialize(serializer)
    }
    
    pub fn deserialize<'de, D, V>(deserializer: D) -> Result<HashMap<Uuid, V>, D::Error>
    where
        D: Deserializer<'de>,
        V: Deserialize<'de>,
    {
        let string_map = HashMap::<String, V>::deserialize(deserializer)?;
        string_map
            .into_iter()
            .map(|(k, v)| {
                Uuid::parse_str(&k)
                    .map(|uuid| (uuid, v))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}

/// Custom serialization for HashSet with UUID values
/// 
/// Serializes UUID values as a JSON array of strings for compatibility
/// while maintaining type safety in Rust code.
pub mod uuid_hashset_serde {
    use super::*;
    
    pub fn serialize<S>(set: &HashSet<Uuid>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let string_vec: Vec<String> = set
            .iter()
            .map(|uuid| uuid.to_string())
            .collect();
        string_vec.serialize(serializer)
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashSet<Uuid>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_vec = Vec::<String>::deserialize(deserializer)?;
        string_vec
            .into_iter()
            .map(|s| Uuid::parse_str(&s).map_err(serde::de::Error::custom))
            .collect()
    }
}

/// Custom serialization for Vec with UUID values
/// 
/// Useful for ordered lists of UUIDs (e.g., agent associations)
#[allow(dead_code)] // TODO: Remove when AgentRegistry uses this
pub mod uuid_vec_serde {
    use super::*;
    
    pub fn serialize<S>(vec: &Vec<Uuid>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let string_vec: Vec<String> = vec
            .iter()
            .map(|uuid| uuid.to_string())
            .collect();
        string_vec.serialize(serializer)
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Uuid>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_vec = Vec::<String>::deserialize(deserializer)?;
        string_vec
            .into_iter()
            .map(|s| Uuid::parse_str(&s).map_err(serde::de::Error::custom))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct TestHashMap {
        #[serde(with = "uuid_hashmap_serde")]
        map: HashMap<Uuid, String>,
    }

    #[derive(Serialize, Deserialize)]
    struct TestHashSet {
        #[serde(with = "uuid_hashset_serde")]
        set: HashSet<Uuid>,
    }

    #[derive(Serialize, Deserialize)]
    struct TestVec {
        #[serde(with = "uuid_vec_serde")]
        vec: Vec<Uuid>,
    }

    #[test]
    fn test_uuid_hashmap_serde() {
        let mut map = HashMap::new();
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        map.insert(uuid1, "value1".to_string());
        map.insert(uuid2, "value2".to_string());

        let test_struct = TestHashMap { map };
        
        // Serialize
        let json = serde_json::to_string(&test_struct).unwrap();
        assert!(json.contains(&uuid1.to_string()));
        assert!(json.contains(&uuid2.to_string()));
        
        // Deserialize
        let deserialized: TestHashMap = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.map.get(&uuid1).unwrap(), "value1");
        assert_eq!(deserialized.map.get(&uuid2).unwrap(), "value2");
    }

    #[test]
    fn test_uuid_hashset_serde() {
        let mut set = HashSet::new();
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        set.insert(uuid1);
        set.insert(uuid2);

        let test_struct = TestHashSet { set };
        
        // Serialize
        let json = serde_json::to_string(&test_struct).unwrap();
        assert!(json.contains(&uuid1.to_string()));
        assert!(json.contains(&uuid2.to_string()));
        
        // Deserialize
        let deserialized: TestHashSet = serde_json::from_str(&json).unwrap();
        assert!(deserialized.set.contains(&uuid1));
        assert!(deserialized.set.contains(&uuid2));
    }

    #[test]
    fn test_uuid_vec_serde() {
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        let vec = vec![uuid1, uuid2];

        let test_struct = TestVec { vec };
        
        // Serialize
        let json = serde_json::to_string(&test_struct).unwrap();
        assert!(json.contains(&uuid1.to_string()));
        assert!(json.contains(&uuid2.to_string()));
        
        // Deserialize
        let deserialized: TestVec = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.vec[0], uuid1);
        assert_eq!(deserialized.vec[1], uuid2);
    }
}