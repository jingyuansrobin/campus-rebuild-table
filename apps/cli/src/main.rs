use app_core::{create_local_project, CreateProjectRequest};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.len() < 4 || args[0] != "init" {
        return Err(usage());
    }

    let blocks_per_meter = if let Some(value) = args.get(4) {
        value
            .parse::<f32>()
            .map_err(|_| format!("invalid blocks_per_meter: {value}\n{}", usage()))?
    } else {
        1.5
    };

    let minecraft_version = args.get(5).cloned().unwrap_or_else(|| "1.20.1".to_owned());

    let request = CreateProjectRequest {
        project_dir: PathBuf::from(&args[1]),
        school_name: args[2].clone(),
        campus_name: args[3].clone(),
        minecraft_version,
        blocks_per_meter,
    };

    let project_dir = request.project_dir.clone();
    let project = create_local_project(request).map_err(|error| error.to_string())?;

    println!(
        "Created MCRebuild project for {} · {} at {} ({} blocks/m)",
        project.campus.school_name,
        project.campus.campus_name,
        project_dir.display(),
        project.generation_scale.blocks_per_meter()
    );

    Ok(())
}

fn usage() -> String {
    "Usage: mcrebuild-cli init <project_dir> <school_name> <campus_name> [blocks_per_meter] [minecraft_version]\nExample: mcrebuild-cli init ./ecnu \"East China Normal University\" \"Zhongbei Campus\" 1.5 1.20.1".to_owned()
}
