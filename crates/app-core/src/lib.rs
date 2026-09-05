use arnis_adapter::{ArnisAdapter, ArnisError, ArnisRunSpec};
use campus_core::{
    CampusBoundary, CampusBoundaryError, CampusCandidate, CampusIdentity, CampusObjectCollection,
    CampusProject, GenerationScale, GenerationScaleError, GenerationTarget, RealityModel,
    Wgs84BoundingBox, Wgs84Coordinate,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

const GENERATION_MANIFEST_SCHEMA_VERSION: u32 = 1;
const ARNIS_GENERATION_MODE: &str = "geo-terrain";

#[derive(Debug, Clone)]
pub struct CreateProjectRequest {
    pub project_dir: PathBuf,
    pub school_name: String,
    pub campus_name: String,
    pub blocks_per_meter: f32,
}

#[derive(Debug, Clone)]
pub struct CreateProjectFromCandidateRequest {
    pub project_dir: PathBuf,
    pub candidate: CampusCandidate,
    pub blocks_per_meter: f32,
}

#[derive(Debug, Clone)]
pub struct SetProjectBoundaryRequest {
    pub project_dir: PathBuf,
    pub vertices: Vec<Wgs84Coordinate>,
}

#[derive(Debug, Clone)]
pub struct GenerateProjectRequest {
    pub project_dir: PathBuf,
    pub arnis_executable: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryEditorContext {
    pub campus_display_name: String,
    pub anchor: Wgs84Coordinate,
    pub existing_boundary: Option<CampusBoundary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationResult {
    pub world_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub generator_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerationManifest {
    pub schema_version: u32,
    pub generator: &'static str,
    pub generator_version: String,
    pub mode: &'static str,
    pub generation_target: GenerationTarget,
    pub world_format: &'static str,
    pub blocks_per_meter: f32,
    pub transport_bbox: Wgs84BoundingBox,
    pub boundary_transport: &'static str,
}

pub fn create_local_project(
    request: CreateProjectRequest,
) -> Result<CampusProject, CreateProjectError> {
    let generation_scale = GenerationScale::try_new(request.blocks_per_meter)?;
    let project = CampusProject::new(
        CampusIdentity::manual(request.school_name, request.campus_name),
        generation_scale,
    );

    persist_new_project(&request.project_dir, project, RealityModel::empty())
}

pub fn create_project_from_candidate(
    request: CreateProjectFromCandidateRequest,
) -> Result<CampusProject, CreateProjectError> {
    let generation_scale = GenerationScale::try_new(request.blocks_per_meter)?;
    let reality = RealityModel::from_candidate(&request.candidate);
    let project = CampusProject::new(request.candidate.identity, generation_scale);

    persist_new_project(&request.project_dir, project, reality)
}

pub fn load_boundary_editor_context(
    project_dir: &Path,
) -> Result<BoundaryEditorContext, LoadProjectError> {
    let project = read_project(project_dir)?;
    let reality_bytes = fs::read(project_dir.join("reality.json"))?;
    let reality: RealityModel = serde_json::from_slice(&reality_bytes)?;
    let anchor = reality
        .sources
        .iter()
        .find_map(|source| source.anchor)
        .ok_or(LoadProjectError::MissingCampusAnchor)?;

    Ok(BoundaryEditorContext {
        campus_display_name: project.campus.display_name,
        anchor,
        existing_boundary: project.boundary,
    })
}

pub fn set_project_boundary(
    request: SetProjectBoundaryRequest,
) -> Result<CampusProject, UpdateProjectError> {
    let boundary = CampusBoundary::try_new(request.vertices)?;
    let project_path = request.project_dir.join("project.json");
    let bytes = fs::read(&project_path)?;
    let mut project: CampusProject = serde_json::from_slice(&bytes)?;
    project.set_boundary(boundary);
    write_json_atomic(&project_path, &project)?;
    Ok(project)
}

pub fn generate_project_with_arnis(
    request: GenerateProjectRequest,
) -> Result<GenerationResult, GenerateProjectError> {
    let project = read_project(&request.project_dir)?;
    ensure_supported_generation_target(project.generation_target)?;

    let generated_dir = request.project_dir.join("generated");
    let final_world_dir = generated_dir.join("world");
    if final_world_dir.exists() {
        return Err(GenerateProjectError::OutputAlreadyExists(final_world_dir));
    }

    let cache_dir = request.project_dir.join("cache");
    fs::create_dir_all(&cache_dir)?;
    fs::create_dir_all(&generated_dir)?;

    // Arnis treats --output-dir as a parent for Java worlds, so stage each run in
    // a unique parent directory and let the adapter resolve the actual child world.
    let staging_root = cache_dir.join(format!("arnis-run-{}", Uuid::new_v4()));
    let run_spec = build_arnis_run_spec(&project, staging_root.clone())?;
    let adapter = ArnisAdapter::new(request.arnis_executable);
    let generator_version = adapter.probe_version()?;

    let run_result = match adapter.run(&run_spec) {
        Ok(result) => result,
        Err(error) => {
            remove_path_if_present(&staging_root);
            return Err(error.into());
        }
    };

    let manifest = GenerationManifest {
        schema_version: GENERATION_MANIFEST_SCHEMA_VERSION,
        generator: "arnis",
        generator_version: generator_version.clone(),
        mode: ARNIS_GENERATION_MODE,
        generation_target: project.generation_target,
        world_format: "java_anvil",
        blocks_per_meter: project.generation_scale.blocks_per_meter(),
        transport_bbox: run_spec.bbox,
        boundary_transport: "polygon_bounding_box",
    };
    let staging_manifest = run_result.world_dir.join(".mcrebuild-generation.json");
    if let Err(error) = write_json_atomic(&staging_manifest, &manifest) {
        remove_path_if_present(&staging_root);
        return Err(error.into());
    }

    if let Err(error) = fs::rename(&run_result.world_dir, &final_world_dir) {
        remove_path_if_present(&staging_root);
        return Err(GenerateProjectError::Io(error));
    }
    remove_path_if_present(&staging_root);

    Ok(GenerationResult {
        manifest_path: final_world_dir.join(".mcrebuild-generation.json"),
        world_dir: final_world_dir,
        generator_version,
    })
}

fn ensure_supported_generation_target(target: GenerationTarget) -> Result<(), GenerateProjectError> {
    match target {
        GenerationTarget::MinecraftJava => Ok(()),
    }
}

fn build_arnis_run_spec(
    project: &CampusProject,
    output_dir: PathBuf,
) -> Result<ArnisRunSpec, GenerateProjectError> {
    let boundary = project
        .boundary
        .as_ref()
        .ok_or(GenerateProjectError::MissingBoundary)?;

    Ok(ArnisRunSpec {
        output_dir,
        bbox: boundary.bounding_box(),
        scale: project.generation_scale,
    })
}

fn read_project(project_dir: &Path) -> Result<CampusProject, LoadProjectError> {
    let project_bytes = fs::read(project_dir.join("project.json"))?;
    Ok(serde_json::from_slice(&project_bytes)?)
}

fn persist_new_project(
    project_dir: &Path,
    project: CampusProject,
    reality: RealityModel,
) -> Result<CampusProject, CreateProjectError> {
    let target_existed = project_dir.exists();
    ensure_target_is_empty_directory(project_dir)?;

    if !target_existed {
        fs::create_dir_all(project_dir)?;
    }

    let result = create_project_contents(project_dir, &project, &reality);

    if result.is_err() && !target_existed {
        let _ = fs::remove_dir_all(project_dir);
    }

    result.map(|()| project)
}

fn create_project_contents(
    project_dir: &Path,
    project: &CampusProject,
    reality: &RealityModel,
) -> Result<(), CreateProjectError> {
    fs::create_dir(project_dir.join("generated"))?;
    fs::create_dir(project_dir.join("cache"))?;

    write_json_atomic(&project_dir.join("project.json"), project)?;
    write_json_atomic(&project_dir.join("reality.json"), reality)?;
    write_json_atomic(
        &project_dir.join("objects.json"),
        &CampusObjectCollection::empty(),
    )?;

    Ok(())
}

fn ensure_target_is_empty_directory(path: &Path) -> Result<(), CreateProjectError> {
    if !path.exists() {
        return Ok(());
    }

    if !path.is_dir() {
        return Err(CreateProjectError::TargetIsNotDirectory(path.to_path_buf()));
    }

    if fs::read_dir(path)?.next().transpose()?.is_some() {
        return Err(CreateProjectError::TargetNotEmpty(path.to_path_buf()));
    }

    Ok(())
}

fn remove_path_if_present(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else if path.exists() {
        let _ = fs::remove_file(path);
    }
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), ProjectFileWriteError> {
    let temp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(&temp_path, bytes)?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum ProjectFileWriteError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum CreateProjectError {
    #[error(transparent)]
    InvalidGenerationScale(#[from] GenerationScaleError),
    #[error("project target already contains files: {0}")]
    TargetNotEmpty(PathBuf),
    #[error("project target exists but is not a directory: {0}")]
    TargetIsNotDirectory(PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    WriteJson(#[from] ProjectFileWriteError),
}

#[derive(Debug, Error)]
pub enum LoadProjectError {
    #[error("project has no geographic anchor; recreate it from a campus search result")]
    MissingCampusAnchor,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum UpdateProjectError {
    #[error(transparent)]
    InvalidBoundary(#[from] CampusBoundaryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    WriteJson(#[from] ProjectFileWriteError),
}

#[derive(Debug, Error)]
pub enum GenerateProjectError {
    #[error("project has no confirmed campus boundary")]
    MissingBoundary,
    #[error("generated world already exists at {0}; MCRebuild will not overwrite it")]
    OutputAlreadyExists(PathBuf),
    #[error(transparent)]
    Generator(#[from] ArnisError),
    #[error(transparent)]
    LoadProject(#[from] LoadProjectError),
    #[error(transparent)]
    WriteJson(#[from] ProjectFileWriteError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use campus_core::{CampusSourceReference, GenerationTarget, Wgs84Coordinate};
    use std::fs;

    fn temporary_test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mcrebuild-{label}-{}", Uuid::new_v4()))
    }

    fn request(project_dir: PathBuf) -> CreateProjectRequest {
        CreateProjectRequest {
            project_dir,
            school_name: "East China Normal University".into(),
            campus_name: "Zhongbei Campus".into(),
            blocks_per_meter: 1.5,
        }
    }

    fn point(longitude: f64, latitude: f64) -> Wgs84Coordinate {
        Wgs84Coordinate::try_new(longitude, latitude).unwrap()
    }

    fn candidate() -> CampusCandidate {
        CampusCandidate {
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
        }
    }

    fn valid_boundary_vertices() -> Vec<Wgs84Coordinate> {
        vec![
            point(121.3980, 31.2220),
            point(121.4140, 31.2220),
            point(121.4140, 31.2340),
            point(121.3980, 31.2340),
        ]
    }

    fn create_candidate_project(project_dir: &Path) -> CampusProject {
        create_project_from_candidate(CreateProjectFromCandidateRequest {
            project_dir: project_dir.to_path_buf(),
            candidate: candidate(),
            blocks_per_meter: 1.5,
        })
        .expect("create candidate project")
    }

    #[test]
    fn creates_local_project_layout_and_round_trips_project_json() {
        let project_dir = temporary_test_path("create");
        let created = create_local_project(request(project_dir.clone())).expect("create project");

        for entry in [
            "project.json",
            "reality.json",
            "objects.json",
            "generated",
            "cache",
        ] {
            assert!(project_dir.join(entry).exists(), "missing {entry}");
        }

        let project_json = fs::read(project_dir.join("project.json")).expect("read project.json");
        let restored: CampusProject =
            serde_json::from_slice(&project_json).expect("parse project.json");
        assert_eq!(created, restored);
        assert_eq!(created.generation_target, GenerationTarget::MinecraftJava);

        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }

    #[test]
    fn candidate_project_persists_source_and_anchor_in_reality_model() {
        let project_dir = temporary_test_path("candidate");
        let selected = candidate();
        let created = create_project_from_candidate(CreateProjectFromCandidateRequest {
            project_dir: project_dir.clone(),
            candidate: selected.clone(),
            blocks_per_meter: 1.5,
        })
        .expect("create candidate project");

        assert_eq!(created.campus, selected.identity);

        let reality_json = fs::read(project_dir.join("reality.json")).expect("read reality.json");
        let reality: RealityModel =
            serde_json::from_slice(&reality_json).expect("parse reality.json");
        assert_eq!(reality.sources[0].reference, "B001");
        assert_eq!(reality.sources[0].anchor, Some(selected.anchor));

        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }

    #[test]
    fn boundary_editor_context_is_loaded_by_app_core() {
        let project_dir = temporary_test_path("editor-context");
        create_candidate_project(&project_dir);
        set_project_boundary(SetProjectBoundaryRequest {
            project_dir: project_dir.clone(),
            vertices: valid_boundary_vertices(),
        })
        .expect("set boundary");

        let context = load_boundary_editor_context(&project_dir).expect("load editor context");
        assert_eq!(context.campus_display_name, "华东师范大学(普陀校区)");
        assert_eq!(context.anchor, point(121.400, 31.226));
        assert!(context.existing_boundary.is_some());

        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }

    #[test]
    fn manual_project_without_anchor_cannot_open_map_editor() {
        let project_dir = temporary_test_path("missing-anchor");
        create_local_project(request(project_dir.clone())).expect("create project");

        let error = load_boundary_editor_context(&project_dir).unwrap_err();
        assert!(matches!(error, LoadProjectError::MissingCampusAnchor));

        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }

    #[test]
    fn setting_boundary_only_rewrites_project_json() {
        let project_dir = temporary_test_path("boundary");
        create_candidate_project(&project_dir);

        let reality_before = fs::read(project_dir.join("reality.json")).unwrap();
        let objects_before = fs::read(project_dir.join("objects.json")).unwrap();

        let updated = set_project_boundary(SetProjectBoundaryRequest {
            project_dir: project_dir.clone(),
            vertices: valid_boundary_vertices(),
        })
        .expect("set boundary");

        assert!(updated.boundary.is_some());
        assert_eq!(
            fs::read(project_dir.join("reality.json")).unwrap(),
            reality_before
        );
        assert_eq!(
            fs::read(project_dir.join("objects.json")).unwrap(),
            objects_before
        );

        let restored: CampusProject =
            serde_json::from_slice(&fs::read(project_dir.join("project.json")).unwrap()).unwrap();
        assert_eq!(updated, restored);

        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }

    #[test]
    fn invalid_boundary_does_not_rewrite_project() {
        let project_dir = temporary_test_path("invalid-boundary");
        create_local_project(request(project_dir.clone())).expect("create project");
        let project_before = fs::read(project_dir.join("project.json")).unwrap();

        let result = set_project_boundary(SetProjectBoundaryRequest {
            project_dir: project_dir.clone(),
            vertices: vec![point(121.4, 31.2), point(121.41, 31.21)],
        });

        assert!(result.is_err());
        assert_eq!(
            fs::read(project_dir.join("project.json")).unwrap(),
            project_before
        );
        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }

    #[test]
    fn generation_requires_a_confirmed_boundary_before_launching_arnis() {
        let project_dir = temporary_test_path("generation-no-boundary");
        create_candidate_project(&project_dir);

        let error = generate_project_with_arnis(GenerateProjectRequest {
            project_dir: project_dir.clone(),
            arnis_executable: PathBuf::from("definitely-not-a-real-arnis-binary"),
        })
        .unwrap_err();

        assert!(matches!(error, GenerateProjectError::MissingBoundary));
        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }

    #[test]
    fn generation_spec_uses_authoritative_boundary_bbox_and_project_scale() {
        let mut project = CampusProject::new(
            CampusIdentity::manual("华东师范大学", "普陀校区"),
            GenerationScale::try_new(1.5).unwrap(),
        );
        project.set_boundary(CampusBoundary::try_new(valid_boundary_vertices()).unwrap());
        let spec = build_arnis_run_spec(&project, PathBuf::from("staging-parent")).unwrap();

        assert_eq!(spec.output_dir, PathBuf::from("staging-parent"));
        assert_eq!(spec.scale.blocks_per_meter(), 1.5);
        assert_eq!(spec.bbox.min_longitude, 121.3980);
        assert_eq!(spec.bbox.min_latitude, 31.2220);
        assert_eq!(spec.bbox.max_longitude, 121.4140);
        assert_eq!(spec.bbox.max_latitude, 31.2340);
    }

    #[test]
    fn generation_refuses_to_overwrite_an_existing_world_before_launching_arnis() {
        let project_dir = temporary_test_path("generation-existing-world");
        create_candidate_project(&project_dir);
        set_project_boundary(SetProjectBoundaryRequest {
            project_dir: project_dir.clone(),
            vertices: valid_boundary_vertices(),
        })
        .unwrap();
        fs::create_dir_all(project_dir.join("generated/world")).unwrap();

        let error = generate_project_with_arnis(GenerateProjectRequest {
            project_dir: project_dir.clone(),
            arnis_executable: PathBuf::from("definitely-not-a-real-arnis-binary"),
        })
        .unwrap_err();

        assert!(matches!(error, GenerateProjectError::OutputAlreadyExists(_)));
        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }

    #[test]
    fn refuses_to_overwrite_non_empty_target() {
        let project_dir = temporary_test_path("non-empty");
        fs::create_dir_all(&project_dir).expect("create test directory");
        fs::write(project_dir.join("keep.txt"), b"do not overwrite").expect("seed target");

        let error = create_local_project(request(project_dir.clone())).expect_err("must refuse");
        assert!(matches!(error, CreateProjectError::TargetNotEmpty(_)));
        assert_eq!(
            fs::read(project_dir.join("keep.txt")).expect("read original file"),
            b"do not overwrite"
        );

        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }
}
