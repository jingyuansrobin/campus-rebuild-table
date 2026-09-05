use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const PROJECT_SCHEMA_VERSION: u32 = 3;
pub const REALITY_SCHEMA_VERSION: u32 = 3;
pub const OBJECTS_SCHEMA_VERSION: u32 = 1;

const EARTH_RADIUS_M: f64 = 6_371_008.8;
const MIN_BOUNDARY_AREA_M2: f64 = 100.0;
const GEOMETRY_EPSILON: f64 = 1e-9;

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

/// Provider-neutral geographic coordinate used by MCRebuild core.
///
/// The coordinate reference system is always WGS-84. Provider adapters such as
/// AMap must normalize their native coordinates before constructing this type.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Wgs84Coordinate {
    longitude: f64,
    latitude: f64,
}

impl Wgs84Coordinate {
    pub fn try_new(longitude: f64, latitude: f64) -> Result<Self, Wgs84CoordinateError> {
        if !longitude.is_finite()
            || !latitude.is_finite()
            || !(-180.0..=180.0).contains(&longitude)
            || !(-90.0..=90.0).contains(&latitude)
        {
            return Err(Wgs84CoordinateError {
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

impl<'de> Deserialize<'de> for Wgs84Coordinate {
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
#[error("invalid WGS-84 coordinate ({longitude}, {latitude})")]
pub struct Wgs84CoordinateError {
    longitude: f64,
    latitude: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CampusBoundary {
    vertices: Vec<Wgs84Coordinate>,
}

impl CampusBoundary {
    pub fn try_new(mut vertices: Vec<Wgs84Coordinate>) -> Result<Self, CampusBoundaryError> {
        if vertices.len() >= 2 && vertices.first() == vertices.last() {
            vertices.pop();
        }

        if vertices.len() < 3 {
            return Err(CampusBoundaryError::InsufficientVertices(vertices.len()));
        }

        for index in 0..vertices.len() {
            let next = (index + 1) % vertices.len();
            if vertices[index] == vertices[next] {
                return Err(CampusBoundaryError::ConsecutiveDuplicateVertex(index));
            }
        }

        let projected = project_to_local_meters(&vertices);
        if let Some((edge_a, edge_b)) = first_self_intersection(&projected) {
            return Err(CampusBoundaryError::SelfIntersecting { edge_a, edge_b });
        }

        let area_m2 = polygon_area_m2(&projected);
        if area_m2 < MIN_BOUNDARY_AREA_M2 {
            return Err(CampusBoundaryError::AreaTooSmall {
                area_m2,
                minimum_m2: MIN_BOUNDARY_AREA_M2,
            });
        }

        Ok(Self { vertices })
    }

    pub fn vertices(&self) -> &[Wgs84Coordinate] {
        &self.vertices
    }

    pub fn area_m2(&self) -> f64 {
        polygon_area_m2(&project_to_local_meters(&self.vertices))
    }

    pub fn bounding_box(&self) -> Wgs84BoundingBox {
        let mut min_longitude = f64::INFINITY;
        let mut min_latitude = f64::INFINITY;
        let mut max_longitude = f64::NEG_INFINITY;
        let mut max_latitude = f64::NEG_INFINITY;

        for vertex in &self.vertices {
            min_longitude = min_longitude.min(vertex.longitude());
            min_latitude = min_latitude.min(vertex.latitude());
            max_longitude = max_longitude.max(vertex.longitude());
            max_latitude = max_latitude.max(vertex.latitude());
        }

        Wgs84BoundingBox {
            min_longitude,
            min_latitude,
            max_longitude,
            max_latitude,
        }
    }
}

impl<'de> Deserialize<'de> for CampusBoundary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawBoundary {
            vertices: Vec<Wgs84Coordinate>,
        }

        let raw = RawBoundary::deserialize(deserializer)?;
        Self::try_new(raw.vertices).map_err(|error| serde::de::Error::custom(error.to_string()))
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum CampusBoundaryError {
    #[error("campus boundary needs at least three effective vertices, got {0}")]
    InsufficientVertices(usize),
    #[error("campus boundary has a consecutive duplicate vertex at index {0}")]
    ConsecutiveDuplicateVertex(usize),
    #[error("campus boundary area is too small: {area_m2:.1} m², minimum {minimum_m2:.0} m²")]
    AreaTooSmall { area_m2: f64, minimum_m2: f64 },
    #[error("campus boundary self-intersects between edges {edge_a} and {edge_b}")]
    SelfIntersecting { edge_a: usize, edge_b: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Wgs84BoundingBox {
    pub min_longitude: f64,
    pub min_latitude: f64,
    pub max_longitude: f64,
    pub max_latitude: f64,
}

impl Wgs84BoundingBox {
    pub fn contains(&self, coordinate: Wgs84Coordinate) -> bool {
        (self.min_longitude..=self.max_longitude).contains(&coordinate.longitude())
            && (self.min_latitude..=self.max_latitude).contains(&coordinate.latitude())
    }
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
    pub anchor: Wgs84Coordinate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampusProject {
    pub schema_version: u32,
    pub id: ProjectId,
    pub campus: CampusIdentity,
    pub minecraft_version: String,
    pub generation_scale: GenerationScale,
    pub boundary: Option<CampusBoundary>,
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
            boundary: None,
        }
    }

    pub fn set_boundary(&mut self, boundary: CampusBoundary) {
        self.boundary = Some(boundary);
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
    pub anchor: Option<Wgs84Coordinate>,
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

#[derive(Debug, Clone, Copy)]
struct ProjectedPoint {
    x: f64,
    y: f64,
}

fn project_to_local_meters(vertices: &[Wgs84Coordinate]) -> Vec<ProjectedPoint> {
    let mean_latitude_rad = vertices
        .iter()
        .map(|point| point.latitude().to_radians())
        .sum::<f64>()
        / vertices.len() as f64;
    let longitude_scale = mean_latitude_rad.cos();

    vertices
        .iter()
        .map(|point| ProjectedPoint {
            x: point.longitude().to_radians() * EARTH_RADIUS_M * longitude_scale,
            y: point.latitude().to_radians() * EARTH_RADIUS_M,
        })
        .collect()
}

fn polygon_area_m2(vertices: &[ProjectedPoint]) -> f64 {
    let mut twice_area = 0.0;
    for index in 0..vertices.len() {
        let next = (index + 1) % vertices.len();
        twice_area += vertices[index].x * vertices[next].y - vertices[next].x * vertices[index].y;
    }
    twice_area.abs() / 2.0
}

fn first_self_intersection(vertices: &[ProjectedPoint]) -> Option<(usize, usize)> {
    let edge_count = vertices.len();
    for edge_a in 0..edge_count {
        let a1 = vertices[edge_a];
        let a2 = vertices[(edge_a + 1) % edge_count];

        for edge_b in (edge_a + 1)..edge_count {
            if edge_b == edge_a + 1 || (edge_a == 0 && edge_b == edge_count - 1) {
                continue;
            }

            let b1 = vertices[edge_b];
            let b2 = vertices[(edge_b + 1) % edge_count];
            if segments_intersect(a1, a2, b1, b2) {
                return Some((edge_a, edge_b));
            }
        }
    }
    None
}

fn segments_intersect(
    a: ProjectedPoint,
    b: ProjectedPoint,
    c: ProjectedPoint,
    d: ProjectedPoint,
) -> bool {
    let ab_c = cross(a, b, c);
    let ab_d = cross(a, b, d);
    let cd_a = cross(c, d, a);
    let cd_b = cross(c, d, b);

    if opposite_signs(ab_c, ab_d) && opposite_signs(cd_a, cd_b) {
        return true;
    }

    (ab_c.abs() <= GEOMETRY_EPSILON && point_on_segment(c, a, b))
        || (ab_d.abs() <= GEOMETRY_EPSILON && point_on_segment(d, a, b))
        || (cd_a.abs() <= GEOMETRY_EPSILON && point_on_segment(a, c, d))
        || (cd_b.abs() <= GEOMETRY_EPSILON && point_on_segment(b, c, d))
}

fn cross(a: ProjectedPoint, b: ProjectedPoint, c: ProjectedPoint) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn opposite_signs(left: f64, right: f64) -> bool {
    (left > GEOMETRY_EPSILON && right < -GEOMETRY_EPSILON)
        || (left < -GEOMETRY_EPSILON && right > GEOMETRY_EPSILON)
}

fn point_on_segment(point: ProjectedPoint, start: ProjectedPoint, end: ProjectedPoint) -> bool {
    point.x >= start.x.min(end.x) - GEOMETRY_EPSILON
        && point.x <= start.x.max(end.x) + GEOMETRY_EPSILON
        && point.y >= start.y.min(end.y) - GEOMETRY_EPSILON
        && point.y <= start.y.max(end.y) + GEOMETRY_EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(longitude: f64, latitude: f64) -> Wgs84Coordinate {
        Wgs84Coordinate::try_new(longitude, latitude).unwrap()
    }

    fn ecnu_like_boundary() -> CampusBoundary {
        CampusBoundary::try_new(vec![
            point(121.3980, 31.2220),
            point(121.4140, 31.2220),
            point(121.4140, 31.2340),
            point(121.3980, 31.2340),
        ])
        .unwrap()
    }

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
            assert!(Wgs84Coordinate::try_new(longitude, latitude).is_err());
        }
    }

    #[test]
    fn explicitly_closed_boundary_is_normalized() {
        let first = point(121.3980, 31.2220);
        let boundary = CampusBoundary::try_new(vec![
            first,
            point(121.4140, 31.2220),
            point(121.4140, 31.2340),
            point(121.3980, 31.2340),
            first,
        ])
        .unwrap();

        assert_eq!(boundary.vertices().len(), 4);
        assert_ne!(boundary.vertices().first(), boundary.vertices().last());
    }

    #[test]
    fn bow_tie_boundary_is_rejected() {
        let error = CampusBoundary::try_new(vec![
            point(121.3980, 31.2220),
            point(121.4140, 31.2340),
            point(121.4140, 31.2220),
            point(121.3980, 31.2340),
        ])
        .unwrap_err();

        assert!(matches!(
            error,
            CampusBoundaryError::SelfIntersecting { .. }
        ));
    }

    #[test]
    fn tiny_boundary_is_rejected() {
        let error = CampusBoundary::try_new(vec![
            point(121.4000000, 31.2200000),
            point(121.4000010, 31.2200000),
            point(121.4000000, 31.2200010),
        ])
        .unwrap_err();

        assert!(matches!(error, CampusBoundaryError::AreaTooSmall { .. }));
    }

    #[test]
    fn bounding_box_contains_every_boundary_vertex() {
        let boundary = ecnu_like_boundary();
        let bounding_box = boundary.bounding_box();
        assert!(boundary
            .vertices()
            .iter()
            .copied()
            .all(|vertex| bounding_box.contains(vertex)));
    }

    #[test]
    fn project_round_trip_revalidates_boundary() {
        let mut project = CampusProject::new(
            CampusIdentity::manual("华东师范大学", "普陀校区"),
            "1.20.1",
            GenerationScale::try_new(1.5).unwrap(),
        );
        project.set_boundary(ecnu_like_boundary());

        let json = serde_json::to_vec(&project).unwrap();
        let restored: CampusProject = serde_json::from_slice(&json).unwrap();
        assert_eq!(project, restored);
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
            anchor: point(121.400, 31.226),
        };

        let reality = RealityModel::from_candidate(&candidate);
        assert_eq!(reality.sources.len(), 1);
        assert_eq!(reality.sources[0].reference, "B001");
        assert_eq!(reality.sources[0].anchor, Some(candidate.anchor));
    }
}
