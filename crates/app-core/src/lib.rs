use campus_core::{
    CampusCandidate, CampusIdentity, CampusObjectCollection, CampusProject, GenerationScale,
    GenerationScaleError, RealityModel,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct CreateProjectRequest {
    pub project_dir: PathBuf,
    pub school_name: String,
    pub campus_name: String,
    pub minecraft_version: String,
    pub blocks_per_meter: f32,
}

#[derive(Debug, Clone)]
pub struct CreateProjectFromCandidateRequest {
    pub project_dir: PathBuf,
    pub candidate: CampusCandidate,
    pub minecraft_version: String,
    pub blocks_per_meter: f32,
}

pub fn create_local_project(
    request: CreateProjectRequest,
) -> Result<CampusProject, CreateProjectError> {
    let generation_scale = GenerationScale::try_new(request.blocks_per_meter)?;
    let project = CampusProject::new(
        CampusIdentity::manual(request.school_name, request.campus_name),
        request.minecraft_version,
        generation_scale,
    );

    persist_new_project(&request.project_dir, project, RealityModel::empty())
}

pub fn create_project_from_candidate(
    request: CreateProjectFromCandidateRequest,
) -> Result<CampusProject, CreateProjectError> {
    let generation_scale = GenerationScale::try_new(request.blocks_per_meter)?;
    let reality = RealityModel::from_candidate(&request.candidate);
    let project = CampusProject::new(
        request.candidate.identity,
        request.minecraft_version,
        generation_scale,
    );

    persist_new_project(&request.project_dir, project, reality)
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

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), CreateProjectError> {
    let temp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(&temp_path, bytes)?;
    fs::rename(&temp_path, path)?;
    Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use campus_core::{CampusSourceReference, GeoCoordinate};
    use std::fs;
    use uuid::Uuid;

    fn temporary_test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mcrebuild-{label}-{}", Uuid::new_v4()))
    }

    fn request(project_dir: PathBuf) -> CreateProjectRequest {
        CreateProjectRequest {
            project_dir,
            school_name: "East China Normal University".into(),
            campus_name: "Zhongbei Campus".into(),
            minecraft_version: "1.20.1".into(),
            blocks_per_meter: 1.5,
        }
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
            anchor: GeoCoordinate::try_new(121.406, 31.228).unwrap(),
        }
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

        fs::remove_dir_all(project_dir).expect("cleanup test directory");
    }

    #[test]
    fn candidate_project_persists_source_and_anchor_in_reality_model() {
        let project_dir = temporary_test_path("candidate");
        let selected = candidate();
        let created = create_project_from_candidate(CreateProjectFromCandidateRequest {
            project_dir: project_dir.clone(),
            candidate: selected.clone(),
            minecraft_version: "1.20.1".into(),
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
