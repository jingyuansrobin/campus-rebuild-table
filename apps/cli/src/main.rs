use app_core::{
    create_local_project, create_project_from_candidate, set_project_boundary,
    CreateProjectFromCandidateRequest, CreateProjectRequest, SetProjectBoundaryRequest,
};
use campus_core::Wgs84Coordinate;
use gaode_search::GaodeSearchClient;
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
    let Some(command) = args.first().map(String::as_str) else {
        return Err(usage());
    };

    match command {
        "init" => run_manual_init(&args[1..]),
        "search-campus" => run_search_campus(&args[1..]),
        "init-campus" => run_init_campus(&args[1..]),
        "set-boundary" => run_set_boundary(&args[1..]),
        _ => Err(usage()),
    }
}

fn run_manual_init(args: &[String]) -> Result<(), String> {
    if args.len() < 3 {
        return Err(usage());
    }

    let blocks_per_meter = parse_scale(args.get(3))?;
    let minecraft_version = args.get(4).cloned().unwrap_or_else(|| "1.20.1".to_owned());

    let request = CreateProjectRequest {
        project_dir: PathBuf::from(&args[0]),
        school_name: args[1].clone(),
        campus_name: args[2].clone(),
        minecraft_version,
        blocks_per_meter,
    };

    let project_dir = request.project_dir.clone();
    let project = create_local_project(request).map_err(|error| error.to_string())?;

    println!(
        "Created MCRebuild project for {} at {} ({} blocks/m)",
        project.campus.display_name,
        project_dir.display(),
        project.generation_scale.blocks_per_meter()
    );

    Ok(())
}

fn run_search_campus(args: &[String]) -> Result<(), String> {
    let Some(keyword) = args.first() else {
        return Err(usage());
    };
    let region = args.get(1).map(String::as_str);
    let client = gaode_client()?;
    let candidates = client
        .search_university_campuses(keyword, region)
        .map_err(|error| error.to_string())?;

    if candidates.is_empty() {
        println!("No university campus candidates found.");
        return Ok(());
    }

    for candidate in candidates {
        println!(
            "{}\t{}\t{}\t{},{}",
            candidate.source.external_id,
            candidate.identity.display_name,
            candidate.address,
            candidate.anchor.longitude(),
            candidate.anchor.latitude()
        );
    }

    Ok(())
}

fn run_init_campus(args: &[String]) -> Result<(), String> {
    if args.len() < 3 {
        return Err(usage());
    }

    let project_dir = PathBuf::from(&args[0]);
    let keyword = &args[1];
    let poi_id = &args[2];
    let blocks_per_meter = parse_scale(args.get(3))?;
    let minecraft_version = args.get(4).cloned().unwrap_or_else(|| "1.20.1".to_owned());
    let region = args.get(5).map(String::as_str);

    let client = gaode_client()?;
    let candidates = client
        .search_university_campuses(keyword, region)
        .map_err(|error| error.to_string())?;
    let candidate = candidates
        .into_iter()
        .find(|candidate| candidate.source.external_id == *poi_id)
        .ok_or_else(|| format!("selected POI id was not found in the current search: {poi_id}"))?;

    let project = create_project_from_candidate(CreateProjectFromCandidateRequest {
        project_dir: project_dir.clone(),
        candidate,
        minecraft_version,
        blocks_per_meter,
    })
    .map_err(|error| error.to_string())?;

    println!(
        "Created MCRebuild project for {} at {} ({} blocks/m)",
        project.campus.display_name,
        project_dir.display(),
        project.generation_scale.blocks_per_meter()
    );

    Ok(())
}

fn run_set_boundary(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err(usage());
    }

    let project_dir = PathBuf::from(&args[0]);
    let vertices = parse_boundary_vertices(&args[1])?;
    let project = set_project_boundary(SetProjectBoundaryRequest {
        project_dir,
        vertices,
    })
    .map_err(|error| error.to_string())?;

    let boundary = project
        .boundary
        .as_ref()
        .ok_or_else(|| "project boundary was not persisted".to_owned())?;
    let bbox = boundary.bounding_box();
    println!(
        "Boundary saved: {} vertices, {:.0} m²; Arnis transport bbox={},{},{},{}",
        boundary.vertices().len(),
        boundary.area_m2(),
        bbox.min_latitude,
        bbox.min_longitude,
        bbox.max_latitude,
        bbox.max_longitude
    );

    Ok(())
}

fn parse_boundary_vertices(value: &str) -> Result<Vec<Wgs84Coordinate>, String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (longitude, latitude) = pair
                .split_once(',')
                .ok_or_else(|| format!("invalid boundary pair: {pair}"))?;
            let longitude = longitude
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("invalid longitude in boundary pair: {pair}"))?;
            let latitude = latitude
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("invalid latitude in boundary pair: {pair}"))?;
            Wgs84Coordinate::try_new(longitude, latitude).map_err(|error| error.to_string())
        })
        .collect()
}

fn gaode_client() -> Result<GaodeSearchClient, String> {
    let api_key = env::var("AMAP_WEB_SERVICE_KEY")
        .map_err(|_| "AMAP_WEB_SERVICE_KEY is not configured".to_owned())?;
    GaodeSearchClient::new(api_key).map_err(|error| error.to_string())
}

fn parse_scale(value: Option<&String>) -> Result<f32, String> {
    match value {
        Some(value) => value
            .parse::<f32>()
            .map_err(|_| format!("invalid blocks_per_meter: {value}\n{}", usage())),
        None => Ok(1.5),
    }
}

fn usage() -> String {
    concat!(
        "Usage:\n",
        "  mcrebuild-cli init <project_dir> <school_name> <campus_name> [blocks_per_meter] [minecraft_version]\n",
        "  mcrebuild-cli search-campus <keyword> [region]\n",
        "  mcrebuild-cli init-campus <project_dir> <keyword> <poi_id> [blocks_per_meter] [minecraft_version] [region]\n",
        "  mcrebuild-cli set-boundary <project_dir> \"lon,lat;lon,lat;lon,lat;...\"\n\n",
        "AMap commands require AMAP_WEB_SERVICE_KEY.\n",
        "All boundary coordinates are WGS-84.\n",
        "Example:\n",
        "  mcrebuild-cli set-boundary ./ecnu \"121.398,31.222;121.414,31.222;121.414,31.234;121.398,31.234\""
    )
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_cli_parser_accepts_wgs84_pairs() {
        let vertices = parse_boundary_vertices(
            "121.398,31.222;121.414,31.222;121.414,31.234;121.398,31.234",
        )
        .unwrap();
        assert_eq!(vertices.len(), 4);
        assert_eq!(vertices[0].longitude(), 121.398);
    }

    #[test]
    fn boundary_cli_parser_rejects_bad_pair() {
        assert!(parse_boundary_vertices("121.398;121.414,31.222").is_err());
    }
}
