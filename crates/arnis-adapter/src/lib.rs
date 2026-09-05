use campus_core::{GenerationScale, Wgs84BoundingBox};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArnisAdapter {
    executable: PathBuf,
}

impl ArnisAdapter {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn plan(&self, spec: &ArnisRunSpec) -> ArnisCommandPlan {
        let bbox = spec.bbox;
        let bbox_arg = format!(
            "{},{},{},{}",
            bbox.min_latitude, bbox.min_longitude, bbox.max_latitude, bbox.max_longitude
        );
        let scale_arg = spec.scale.blocks_per_meter().to_string();

        ArnisCommandPlan {
            executable: self.executable.clone(),
            args: vec![
                OsString::from("--output-dir"),
                spec.output_dir.as_os_str().to_owned(),
                OsString::from("--bbox"),
                OsString::from(bbox_arg),
                OsString::from("--scale"),
                OsString::from(scale_arg),
                OsString::from("--mode"),
                OsString::from("geo-terrain"),
            ],
        }
    }

    /// Run Arnis and resolve the Java world directory it creates below `output_dir`.
    ///
    /// Arnis currently treats `--output-dir` as a parent directory for Java worlds and
    /// creates a child such as `Arnis World 1`. This provider-specific behavior is kept
    /// inside the adapter so MCRebuild application code never depends on that naming scheme.
    pub fn run(&self, spec: &ArnisRunSpec) -> Result<ArnisRunResult, ArnisError> {
        fs::create_dir_all(&spec.output_dir).map_err(|source| ArnisError::PrepareOutput {
            output_dir: spec.output_dir.clone(),
            source,
        })?;

        let plan = self.plan(spec);
        let status = Command::new(&plan.executable)
            .args(&plan.args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|source| ArnisError::Launch {
                executable: plan.executable.clone(),
                source,
            })?;

        if !status.success() {
            return Err(ArnisError::NonZeroExit {
                code: status.code(),
            });
        }

        let world_dir = discover_java_world(&spec.output_dir)?;
        Ok(ArnisRunResult { world_dir })
    }

    pub fn probe_version(&self) -> Result<String, ArnisError> {
        let output = Command::new(&self.executable)
            .arg("--version")
            .output()
            .map_err(|source| ArnisError::Launch {
                executable: self.executable.clone(),
                source,
            })?;

        if !output.status.success() {
            return Err(ArnisError::VersionProbeFailed {
                code: output.status.code(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        parse_version_output(&stdout, &stderr).ok_or(ArnisError::UnrecognizedVersionOutput)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArnisRunSpec {
    /// Parent directory into which Arnis creates a Java world directory.
    pub output_dir: PathBuf,
    pub bbox: Wgs84BoundingBox,
    pub scale: GenerationScale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArnisRunResult {
    pub world_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArnisCommandPlan {
    executable: PathBuf,
    args: Vec<OsString>,
}

impl ArnisCommandPlan {
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    pub fn arg_strings(&self) -> Vec<String> {
        self.args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    pub fn contains_arg(&self, arg: impl AsRef<OsStr>) -> bool {
        self.args.iter().any(|value| value == arg.as_ref())
    }
}

fn parse_version_output(stdout: &str, stderr: &str) -> Option<String> {
    parse_version_text(stdout).or_else(|| parse_version_text(stderr))
}

fn parse_version_text(text: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let version = line.trim().strip_prefix("arnis ")?.trim();
        (!version.is_empty()).then(|| version.to_owned())
    })
}

fn discover_java_world(output_dir: &Path) -> Result<PathBuf, ArnisError> {
    let entries = fs::read_dir(output_dir).map_err(|source| ArnisError::ScanOutput {
        output_dir: output_dir.to_path_buf(),
        source,
    })?;

    let mut worlds = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ArnisError::ScanOutput {
            output_dir: output_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() && path.join("level.dat").is_file() {
            worlds.push(path);
        }
    }

    match worlds.len() {
        1 => Ok(worlds.remove(0)),
        0 => Err(ArnisError::MissingJavaWorld {
            output_dir: output_dir.to_path_buf(),
        }),
        count => Err(ArnisError::MultipleJavaWorlds {
            output_dir: output_dir.to_path_buf(),
            count,
        }),
    }
}

#[derive(Debug, Error)]
pub enum ArnisError {
    #[error("failed to prepare Arnis output directory {output_dir}: {source}")]
    PrepareOutput {
        output_dir: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to launch Arnis executable {executable}: {source}")]
    Launch {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Arnis exited unsuccessfully with code {code:?}")]
    NonZeroExit { code: Option<i32> },
    #[error("Arnis --version exited unsuccessfully with code {code:?}")]
    VersionProbeFailed { code: Option<i32> },
    #[error("Arnis --version output did not contain a recognizable `arnis <version>` line")]
    UnrecognizedVersionOutput,
    #[error("failed to inspect Arnis output directory {output_dir}: {source}")]
    ScanOutput {
        output_dir: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Arnis exited successfully but created no Java world under {output_dir}")]
    MissingJavaWorld { output_dir: PathBuf },
    #[error("Arnis created {count} Java worlds under {output_dir}; expected exactly one")]
    MultipleJavaWorlds { output_dir: PathBuf, count: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn spec() -> ArnisRunSpec {
        ArnisRunSpec {
            output_dir: PathBuf::from("generated/world.partial"),
            bbox: Wgs84BoundingBox {
                min_longitude: 121.398,
                min_latitude: 31.222,
                max_longitude: 121.414,
                max_latitude: 31.234,
            },
            scale: GenerationScale::try_new(1.5).unwrap(),
        }
    }

    fn temporary_test_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("mcrebuild-arnis-{label}-{nonce}"))
    }

    #[test]
    fn command_plan_maps_domain_values_to_current_arnis_cli() {
        let adapter = ArnisAdapter::new("arnis");
        let plan = adapter.plan(&spec());
        let args = plan.arg_strings();

        assert_eq!(plan.executable(), Path::new("arnis"));
        assert_eq!(
            args,
            vec![
                "--output-dir",
                "generated/world.partial",
                "--bbox",
                "31.222,121.398,31.234,121.414",
                "--scale",
                "1.5",
                "--mode",
                "geo-terrain",
            ]
        );
    }

    #[test]
    fn command_plan_does_not_invent_minecraft_version_or_polygon_flags() {
        let plan = ArnisAdapter::new("arnis").plan(&spec());
        assert!(!plan.contains_arg("--minecraft-version"));
        assert!(!plan.contains_arg("--polygon"));
        assert!(plan.contains_arg("--bbox"));
    }

    #[test]
    fn version_parser_extracts_semantic_value_from_release_banner() {
        let stdout = r#"
        ▄████████    ▄████████ ███▄▄▄▄    ▄█     ▄████████

                          version 3.1.0
                https://github.com/louis-e/arnis

arnis 3.1.0
"#;

        assert_eq!(parse_version_output(stdout, ""), Some("3.1.0".to_owned()));
    }

    #[test]
    fn version_parser_accepts_stderr_fallback_and_rejects_unrecognized_output() {
        assert_eq!(
            parse_version_output("", "arnis 9.9.9-test\n"),
            Some("9.9.9-test".to_owned())
        );
        assert_eq!(parse_version_output("version 3.1.0", ""), None);
    }

    #[test]
    fn discovers_world_child_created_under_arnis_output_parent() {
        let output_dir = temporary_test_path("discover");
        let world_dir = output_dir.join("Arnis World 1");
        fs::create_dir_all(&world_dir).unwrap();
        fs::write(world_dir.join("level.dat"), b"fixture").unwrap();

        let discovered = discover_java_world(&output_dir).unwrap();
        assert_eq!(discovered, world_dir);

        fs::remove_dir_all(output_dir).unwrap();
    }

    #[test]
    fn rejects_ambiguous_multiple_world_outputs() {
        let output_dir = temporary_test_path("multiple");
        for name in ["Arnis World 1", "Arnis World 2"] {
            let world_dir = output_dir.join(name);
            fs::create_dir_all(&world_dir).unwrap();
            fs::write(world_dir.join("level.dat"), b"fixture").unwrap();
        }

        let error = discover_java_world(&output_dir).unwrap_err();
        assert!(matches!(
            error,
            ArnisError::MultipleJavaWorlds { count: 2, .. }
        ));

        fs::remove_dir_all(output_dir).unwrap();
    }
}
