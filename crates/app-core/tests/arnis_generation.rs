#![cfg(unix)]

use app_core::{
    create_project_from_candidate, generate_project_with_arnis, set_project_boundary,
    CreateProjectFromCandidateRequest, GenerateProjectRequest, SetProjectBoundaryRequest,
};
use campus_core::{
    CampusCandidate, CampusIdentity, CampusSourceReference, Wgs84Coordinate,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn temporary_test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mcrebuild-integration-{label}-{}", Uuid::new_v4()))
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

fn create_fake_arnis(bin_dir: &Path) -> PathBuf {
    fs::create_dir_all(bin_dir).unwrap();
    let executable = bin_dir.join("fake-arnis");
    fs::write(
        &executable,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "arnis 9.9.9-test"
  exit 0
fi

output_dir=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-dir)
      output_dir="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [ -z "$output_dir" ]; then
  exit 12
fi

mkdir -p "$output_dir/Arnis World 1"
printf 'fake-level-dat' > "$output_dir/Arnis World 1/level.dat"
"#,
    )
    .unwrap();

    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    executable
}

#[test]
fn successful_generation_promotes_one_complete_world_and_records_provenance() {
    let project_dir = temporary_test_path("project");
    let bin_dir = temporary_test_path("bin");
    let fake_arnis = create_fake_arnis(&bin_dir);

    create_project_from_candidate(CreateProjectFromCandidateRequest {
        project_dir: project_dir.clone(),
        candidate: candidate(),
        blocks_per_meter: 1.5,
    })
    .unwrap();
    set_project_boundary(SetProjectBoundaryRequest {
        project_dir: project_dir.clone(),
        vertices: vec![
            point(121.3980, 31.2220),
            point(121.4140, 31.2220),
            point(121.4140, 31.2340),
            point(121.3980, 31.2340),
        ],
    })
    .unwrap();

    let result = generate_project_with_arnis(GenerateProjectRequest {
        project_dir: project_dir.clone(),
        arnis_executable: fake_arnis,
    })
    .unwrap();

    let expected_world = project_dir.join("generated/world");
    assert_eq!(result.world_dir, expected_world);
    assert_eq!(result.generator_version, "arnis 9.9.9-test");
    assert!(result.world_dir.join("level.dat").is_file());
    assert!(result.manifest_path.is_file());

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&result.manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["generator"], "arnis");
    assert_eq!(manifest["generator_version"], "arnis 9.9.9-test");
    assert_eq!(manifest["world_format"], "java_anvil");
    assert_eq!(manifest["boundary_transport"], "polygon_bounding_box");
    assert_eq!(manifest["blocks_per_meter"], 1.5);
    assert_eq!(manifest["transport_bbox"]["min_longitude"], 121.398);
    assert_eq!(manifest["transport_bbox"]["max_latitude"], 31.234);

    let cache_entries = fs::read_dir(project_dir.join("cache")).unwrap().count();
    assert_eq!(cache_entries, 0, "staging directory should be removed");

    fs::remove_dir_all(project_dir).unwrap();
    fs::remove_dir_all(bin_dir).unwrap();
}
