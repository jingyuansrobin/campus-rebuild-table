#![cfg(unix)]

use app_core::{
    create_project_from_candidate, generate_project_with_arnis, generate_project_with_arnis_observed,
    set_project_boundary, CreateProjectFromCandidateRequest, GenerateProjectError,
    GenerateProjectRequest, GenerationCancellationToken, GenerationEvent, GenerationLogStream,
    GenerationStage, SetProjectBoundaryRequest,
};
use campus_core::{CampusCandidate, CampusIdentity, CampusSourceReference, Wgs84Coordinate};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
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

fn make_executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn create_fake_arnis(bin_dir: &Path) -> PathBuf {
    fs::create_dir_all(bin_dir).unwrap();
    let executable = bin_dir.join("fake-arnis");
    make_executable(
        &executable,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "fake banner"
  echo "version 9.9.9-test"
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

echo "[1/7] Fetching data..."
echo "fake stderr diagnostic" >&2
echo "[4/7] Processing data..."
echo "[5/7] Generating area..."
mkdir -p "$output_dir/Arnis World 1"
printf 'fake-level-dat' > "$output_dir/Arnis World 1/level.dat"
echo "[7/7] Saving world..."
"#,
    );
    executable
}

fn create_cancellable_fake_arnis(bin_dir: &Path) -> PathBuf {
    fs::create_dir_all(bin_dir).unwrap();
    let executable = bin_dir.join("fake-arnis-cancellable");
    make_executable(
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

echo "[1/7] Fetching data..."
mkdir -p "$output_dir/Arnis World 1"
printf 'partial' > "$output_dir/Arnis World 1/partial.tmp"
while :; do :; done
"#,
    );
    executable
}

fn create_ready_project(project_dir: &Path) {
    create_project_from_candidate(CreateProjectFromCandidateRequest {
        project_dir: project_dir.to_path_buf(),
        candidate: candidate(),
        blocks_per_meter: 1.5,
    })
    .unwrap();
    set_project_boundary(SetProjectBoundaryRequest {
        project_dir: project_dir.to_path_buf(),
        vertices: vec![
            point(121.3980, 31.2220),
            point(121.4140, 31.2220),
            point(121.4140, 31.2340),
            point(121.3980, 31.2340),
        ],
    })
    .unwrap();
}

#[test]
fn successful_generation_promotes_one_complete_world_and_records_provenance() {
    let project_dir = temporary_test_path("project");
    let bin_dir = temporary_test_path("bin");
    let fake_arnis = create_fake_arnis(&bin_dir);
    create_ready_project(&project_dir);

    let result = generate_project_with_arnis(GenerateProjectRequest {
        project_dir: project_dir.clone(),
        arnis_executable: fake_arnis,
    })
    .unwrap();

    let expected_world = project_dir.join("generated/world");
    assert_eq!(result.world_dir, expected_world);
    assert_eq!(result.generator_version, "9.9.9-test");
    assert!(result.world_dir.join("level.dat").is_file());
    assert!(result.manifest_path.is_file());

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&result.manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["generator"], "arnis");
    assert_eq!(manifest["generator_version"], "9.9.9-test");
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

#[test]
fn observed_generation_emits_provider_neutral_stages_and_raw_logs() {
    let project_dir = temporary_test_path("observed-project");
    let bin_dir = temporary_test_path("observed-bin");
    let fake_arnis = create_fake_arnis(&bin_dir);
    create_ready_project(&project_dir);

    let cancellation = GenerationCancellationToken::new();
    let mut events = Vec::new();
    let result = generate_project_with_arnis_observed(
        GenerateProjectRequest {
            project_dir: project_dir.clone(),
            arnis_executable: fake_arnis,
        },
        &cancellation,
        |event| events.push(event),
    )
    .unwrap();

    assert!(result.world_dir.join("level.dat").is_file());
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                GenerationEvent::Stage(stage) => Some(*stage),
                GenerationEvent::Log { .. } => None,
            })
            .collect::<Vec<_>>(),
        vec![
            GenerationStage::PreparingData,
            GenerationStage::ProcessingMap,
            GenerationStage::GeneratingWorld,
            GenerationStage::SavingWorld,
        ]
    );
    assert!(events.iter().any(|event| matches!(
        event,
        GenerationEvent::Log {
            stream: GenerationLogStream::Stderr,
            line,
        } if line == "fake stderr diagnostic"
    )));

    fs::remove_dir_all(project_dir).unwrap();
    fs::remove_dir_all(bin_dir).unwrap();
}

#[test]
fn cancellation_kills_generation_and_removes_staging_without_publishing_world() {
    let project_dir = temporary_test_path("cancel-project");
    let bin_dir = temporary_test_path("cancel-bin");
    let fake_arnis = create_cancellable_fake_arnis(&bin_dir);
    create_ready_project(&project_dir);

    let cancellation = GenerationCancellationToken::new();
    let worker_cancellation = cancellation.clone();
    let worker_project = project_dir.clone();
    let (event_tx, event_rx) = mpsc::channel();

    let worker = thread::spawn(move || {
        generate_project_with_arnis_observed(
            GenerateProjectRequest {
                project_dir: worker_project,
                arnis_executable: fake_arnis,
            },
            &worker_cancellation,
            |event| {
                let _ = event_tx.send(event);
            },
        )
    });

    let mut saw_preparing = false;
    for _ in 0..10 {
        let event = event_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("fake Arnis should emit a stage before cancellation");
        if event == GenerationEvent::Stage(GenerationStage::PreparingData) {
            saw_preparing = true;
            break;
        }
    }
    assert!(saw_preparing);

    cancellation.cancel();
    let error = worker.join().expect("generation worker should not panic").unwrap_err();
    assert!(matches!(error, GenerateProjectError::Cancelled));
    assert!(!project_dir.join("generated/world").exists());
    assert_eq!(
        fs::read_dir(project_dir.join("cache")).unwrap().count(),
        0,
        "cancelled generation must remove its staging root"
    );

    fs::remove_dir_all(project_dir).unwrap();
    fs::remove_dir_all(bin_dir).unwrap();
}
