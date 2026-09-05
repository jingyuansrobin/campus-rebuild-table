use campus_core::{GenerationScale, Wgs84BoundingBox};
use std::ffi::{OsStr, OsString};
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

    pub fn run(&self, spec: &ArnisRunSpec) -> Result<(), ArnisError> {
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
            return Err(ArnisError::NonZeroExit { code: status.code() });
        }

        Ok(())
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

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let version = if stdout.is_empty() { stderr } else { stdout };
        if version.is_empty() {
            return Err(ArnisError::EmptyVersion);
        }
        Ok(version)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArnisRunSpec {
    pub output_dir: PathBuf,
    pub bbox: Wgs84BoundingBox,
    pub scale: GenerationScale,
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

#[derive(Debug, Error)]
pub enum ArnisError {
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
    #[error("Arnis --version returned no version text")]
    EmptyVersion,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
