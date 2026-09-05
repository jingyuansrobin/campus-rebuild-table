use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const PROJECT_SCHEMA_VERSION: u32 = 2;
pub const REALITY_SCHEMA_VERSION: u32 = 2;
pub const OBJECTS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(transparent)]
pub struct GenerationScale(f32);

impl GenerationScale {
    pub const MIN: f32 = 1.0;
    pub const MAX: f32 = 2.5;

    pub fn try_new(blocks_per_meter: f32) -> Result<Self, GenerationScaleError> {
        if !blocks_per_meter.is_finite() || !(Self::MIN..=Self::MAX).contains(&blocks_per_meter) {
            return Err(GenerationScaleError { blocks_per_meter });
        }

        Ok(Self(blocks_per_meter))
    }

    pub fn blocks_per_meter(self) -> f32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for GenerationScale {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f32::deserialize(deserializer)?;
        Self::try_new(value).map_err(|error| serde::de::Error::custom(error.to_string()))
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq)]
#[error("generation scale must be between 1.0 and 2.5 blocks/m, got {blocks_per_meter}")]
pub struct GenerationScaleError {
    blocks_per_meter: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GeoCoordinate {
    longitude: f64,
    latitude: f64,
}

impl GeoCoordinate {
    pub fn try_new(longitude: f64, latitude: f64) -> Result<Self, GeoCoordinateError> {
        if !longitude.is_finite()
            || !latitude.is_finite()
            || !(-180.0..=180.0).contains(&longitude)
            || !(-90.0..=90.0).contains(&latitude)
        {
            return Err(GeoCoordinateError {
                longitude,
                latitude,
            });
        }

        Ok(Self {
            longitude,
            latitude,
        })
    }

    pub fn longitude(self) -> f64 {
        self.longitude
    }

    pub fn latitude(self) -> f64 {
        self.latitude
    }
}

impl<'de> Deserialize<'de> for GeoCoordinate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawCoordinate {
            longitude: f64,
            latitude: f64,
        }

        let raw = RawCoordinate::deserialize(deserializer)?;
        Self::try_new(raw.longitude, raw.latitude)
            .map_err(|error| serde::de::Error::custom(error.to_string()))
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq)]
#[error("invalid geographic coordinate ({longitude}, {latitude})")]
pub struct GeoCoordinateError {
    longitude: f64,
    latitude: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusIdentity {
    pub school_name: String,
    pub campus_name: Option<String>,
    pub display_name: String,
}

impl CampusIdentity {
    pub fn manual(school_name: impl Into<String>, campus_name: impl Into<String>) -> Self {
        let school_name = school_name.into();
        let campus_name = campus_name.into();
        let display_name = if campus_name == school_name {
            school_name.clone()
        } else {
            format!("{school_name} · {campus_name}")
        };

        Self {
            school_name,
            campus_name: Some(campus_name),
            display_name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusSourceReference {
    pub provider: String,
    pub external_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampusCandidate {
    pub source: CampusSourceReference,
    pub identity: CampusIdentity,
    pub address: String,
    pub anchor: GeoCoordinate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampusProject {
    pub schema_version: u32,
    pub id: ProjectId,
    pub campus: CampusIdentity,
    pub minecraft_version: String,
    pub generation_scale: GenerationScale,
}

impl CampusProject {
    pub fn new(
        campus: CampusIdentity,
        minecraft_version: impl Into<String>,
        generation_scale: GenerationScale,
    ) -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            id: ProjectId::new(),
            campus,
            minecraft_version: minecraft_version.into(),
            generation_scale,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealityModel {
    pub schema_version: u32,
    pub sources: Vec<RealitySource>,
}

impl RealityModel {
    pub fn empty() -> Self {
        Self {
            schema_version: REALITY_SCHEMA_VERSION,
            sources: Vec::new(),
        }
    }

    pub fn from_candidate(candidate: &CampusCandidate) -> Self {
        Self {
            schema_version: REALITY_SCHEMA_VERSION,
            sources: vec![RealitySource {
                kind: candidate.source.provider.clone(),
                reference: candidate.source.external_id.clone(),
                display_name: Some(candidate.identity.display_name.clone()),
                address: Some(candidate.address.clone()),
                anchor: Some(candidate.anchor),
            }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealitySource {
    pub kind: String,
    pub reference: String,
    pub display_name: Option<String>,
    pub address: Option<String>,
    pub anchor: Option<GeoCoordinate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusObjectCollection {
    pub schema_version: u32,
    pub objects: Vec<CampusObject>,
}

impl CampusObjectCollection {
    pub fn empty() -> Self {
        Self {
            schema_version: OBJECTS_SCHEMA_VERSION,
            objects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusObject {
    pub id: Uuid,
    pub object_type: CampusObjectType,
    pub name: Option<String>,
    pub source_reference: Option<String>,
    pub style_reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusObjectType {
    Building,
    Road,
    Water,
    Vegetation,
    SportsFacility,
    Plaza,
    Landmark,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_scale_accepts_product_range() {
        for value in [1.0, 1.5, 2.0, 2.5] {
            let scale = GenerationScale::try_new(value).expect("valid product scale");
            assert_eq!(scale.blocks_per_meter(), value);
        }
    }

    #[test]
    fn generation_scale_rejects_out_of_range_and_non_finite_values() {
        for value in [0.99, 2.51, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(GenerationScale::try_new(value).is_err());
        }
    }

    #[test]
    fn coordinate_rejects_non_finite_and_out_of_range_values() {
        for (longitude, latitude) in [
            (181.0, 31.0),
            (121.0, 91.0),
            (f64::NAN, 31.0),
            (121.0, f64::INFINITY),
        ] {
            assert!(GeoCoordinate::try_new(longitude, latitude).is_err());
        }
    }

    #[test]
    fn reality_model_preserves_selected_candidate_source() {
        let candidate = CampusCandidate {
            source: CampusSourceReference {
                provider: "gaode".into(),
                external_id: "B001".into(),
            },
            identity: CampusIdentity {
                school_name: "华东师范大学".into(),
                campus_name: Some("普陀校区".into()),
                display_name: "华东师范大学(普陀校区)".into(),
            },
            address: "中山北路3663号".into(),
            anchor: GeoCoordinate::try_new(121.406, 31.228).unwrap(),
        };

        let reality = RealityModel::from_candidate(&candidate);
        assert_eq!(reality.sources.len(), 1);
        assert_eq!(reality.sources[0].reference, "B001");
        assert_eq!(reality.sources[0].anchor, Some(candidate.anchor));
    }
}
