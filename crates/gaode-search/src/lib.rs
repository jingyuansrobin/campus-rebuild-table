use campus_core::{CampusCandidate, CampusIdentity, CampusSourceReference, Wgs84Coordinate};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::f64::consts::PI;
use thiserror::Error;

const GAODE_TEXT_SEARCH_ENDPOINT: &str = "https://restapi.amap.com/v3/place/text";
const HIGHER_EDUCATION_TYPECODE: &str = "141201";
const MAX_RESULTS: usize = 20;
const KRASOVSKY_A: f64 = 6_378_245.0;
const KRASOVSKY_EE: f64 = 0.006_693_421_622_965_943;
const GCJ_INVERSE_ITERATIONS: usize = 4;

pub struct GaodeSearchClient {
    api_key: String,
}

impl GaodeSearchClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self, SearchError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(SearchError::MissingApiKey);
        }

        Ok(Self { api_key })
    }

    pub fn search_university_campuses(
        &self,
        keyword: &str,
        region: Option<&str>,
    ) -> Result<Vec<CampusCandidate>, SearchError> {
        let url = build_search_url(&self.api_key, keyword, region)?;
        let response = ureq::get(&url).call().map_err(|error| match error {
            ureq::Error::Status(code, _) => SearchError::HttpStatus(code),
            ureq::Error::Transport(_) => SearchError::Transport,
        })?;
        let body = response.into_string().map_err(|_| SearchError::Transport)?;

        parse_search_response(&body)
    }
}

fn build_search_url(
    api_key: &str,
    keyword: &str,
    region: Option<&str>,
) -> Result<String, SearchError> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Err(SearchError::BlankKeyword);
    }

    let mut url = format!(
        "{GAODE_TEXT_SEARCH_ENDPOINT}?key={}&keywords={}&types={HIGHER_EDUCATION_TYPECODE}&offset={MAX_RESULTS}&page=1&extensions=base&output=json",
        urlencoding::encode(api_key),
        urlencoding::encode(keyword)
    );

    if let Some(region) = region.map(str::trim).filter(|value| !value.is_empty()) {
        url.push_str("&city=");
        url.push_str(&urlencoding::encode(region));
        url.push_str("&citylimit=false");
    }

    Ok(url)
}

pub fn parse_search_response(json: &str) -> Result<Vec<CampusCandidate>, SearchError> {
    let response: RawResponse = serde_json::from_str(json)
        .map_err(|error| SearchError::MalformedResponse(error.to_string()))?;

    if response.status != "1" {
        return Err(SearchError::ServiceRejected {
            info: response.info,
            infocode: response.infocode,
        });
    }

    let mut seen_ids = HashSet::new();
    let mut candidates = Vec::new();

    for raw in response.pois {
        if raw.typecode != HIGHER_EDUCATION_TYPECODE || raw.id.trim().is_empty() {
            continue;
        }
        if !seen_ids.insert(raw.id.clone()) {
            continue;
        }

        let Some(anchor) = parse_gcj02_location_as_wgs84(&raw.location) else {
            continue;
        };
        let display_name = raw.name.trim();
        if display_name.is_empty() {
            continue;
        }

        let identity = parse_campus_identity(display_name);
        let address = compose_address(&raw);
        let external_id = raw.id;

        candidates.push(CampusCandidate {
            source: CampusSourceReference {
                provider: "gaode".to_owned(),
                external_id,
            },
            identity,
            address,
            anchor,
        });
    }

    Ok(candidates)
}

#[derive(Debug, Deserialize)]
struct RawResponse {
    #[serde(default)]
    status: String,
    #[serde(default)]
    info: String,
    #[serde(default)]
    infocode: String,
    #[serde(default)]
    pois: Vec<RawPoi>,
}

#[derive(Debug, Deserialize)]
struct RawPoi {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    typecode: String,
    #[serde(default)]
    address: Value,
    #[serde(default)]
    location: Value,
    #[serde(default)]
    pname: Value,
    #[serde(default)]
    cityname: Value,
    #[serde(default)]
    adname: Value,
}

fn parse_gcj02_location_as_wgs84(value: &Value) -> Option<Wgs84Coordinate> {
    let text = value.as_str()?.trim();
    let (longitude, latitude) = text.split_once(',')?;
    let gcj_longitude = longitude.trim().parse::<f64>().ok()?;
    let gcj_latitude = latitude.trim().parse::<f64>().ok()?;
    if !gcj_longitude.is_finite()
        || !gcj_latitude.is_finite()
        || !(-180.0..=180.0).contains(&gcj_longitude)
        || !(-90.0..=90.0).contains(&gcj_latitude)
    {
        return None;
    }

    let (wgs_longitude, wgs_latitude) = gcj02_to_wgs84(gcj_longitude, gcj_latitude);
    Wgs84Coordinate::try_new(wgs_longitude, wgs_latitude).ok()
}

fn gcj02_to_wgs84(gcj_longitude: f64, gcj_latitude: f64) -> (f64, f64) {
    if out_of_china(gcj_longitude, gcj_latitude) {
        return (gcj_longitude, gcj_latitude);
    }

    let mut wgs_longitude = gcj_longitude;
    let mut wgs_latitude = gcj_latitude;
    for _ in 0..GCJ_INVERSE_ITERATIONS {
        let (estimated_gcj_longitude, estimated_gcj_latitude) =
            wgs84_to_gcj02(wgs_longitude, wgs_latitude);
        wgs_longitude -= estimated_gcj_longitude - gcj_longitude;
        wgs_latitude -= estimated_gcj_latitude - gcj_latitude;
    }
    (wgs_longitude, wgs_latitude)
}

fn wgs84_to_gcj02(longitude: f64, latitude: f64) -> (f64, f64) {
    if out_of_china(longitude, latitude) {
        return (longitude, latitude);
    }

    let delta_latitude = transform_latitude(longitude - 105.0, latitude - 35.0);
    let delta_longitude = transform_longitude(longitude - 105.0, latitude - 35.0);
    let latitude_radians = latitude.to_radians();
    let sin_latitude = latitude_radians.sin();
    let magic = 1.0 - KRASOVSKY_EE * sin_latitude * sin_latitude;
    let sqrt_magic = magic.sqrt();
    let adjusted_latitude = (delta_latitude * 180.0)
        / ((KRASOVSKY_A * (1.0 - KRASOVSKY_EE)) / (magic * sqrt_magic) * PI);
    let adjusted_longitude =
        (delta_longitude * 180.0) / (KRASOVSKY_A / sqrt_magic * latitude_radians.cos() * PI);

    (longitude + adjusted_longitude, latitude + adjusted_latitude)
}

fn out_of_china(longitude: f64, latitude: f64) -> bool {
    !(72.004..=137.8347).contains(&longitude) || !(0.8293..=55.8271).contains(&latitude)
}

fn transform_latitude(x: f64, y: f64) -> f64 {
    let mut result =
        -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * x.abs().sqrt();
    result += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    result += (20.0 * (y * PI).sin() + 40.0 * (y / 3.0 * PI).sin()) * 2.0 / 3.0;
    result += (160.0 * (y / 12.0 * PI).sin() + 320.0 * (y * PI / 30.0).sin()) * 2.0 / 3.0;
    result
}

fn transform_longitude(x: f64, y: f64) -> f64 {
    let mut result =
        300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * x.abs().sqrt();
    result += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    result += (20.0 * (x * PI).sin() + 40.0 * (x / 3.0 * PI).sin()) * 2.0 / 3.0;
    result += (150.0 * (x / 12.0 * PI).sin() + 300.0 * (x / 30.0 * PI).sin()) * 2.0 / 3.0;
    result
}

fn compose_address(raw: &RawPoi) -> String {
    let mut parts = Vec::new();
    for value in [&raw.pname, &raw.cityname, &raw.adname, &raw.address] {
        let text = flatten_text(value);
        if !text.is_empty() && !parts.iter().any(|existing| existing == &text) {
            parts.push(text);
        }
    }
    parts.join("")
}

fn flatten_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_owned(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn parse_campus_identity(display_name: &str) -> CampusIdentity {
    if let Some((school_name, campus_name)) = trailing_campus_parentheses(display_name) {
        return CampusIdentity {
            school_name,
            campus_name: Some(campus_name),
            display_name: display_name.to_owned(),
        };
    }

    if let Some((school_name, campus_name)) = trailing_campus_separator(display_name) {
        return CampusIdentity {
            school_name,
            campus_name: Some(campus_name),
            display_name: display_name.to_owned(),
        };
    }

    CampusIdentity {
        school_name: display_name.to_owned(),
        campus_name: None,
        display_name: display_name.to_owned(),
    }
}

fn trailing_campus_parentheses(display_name: &str) -> Option<(String, String)> {
    for (open, close) in [('（', '）'), ('(', ')')] {
        if !display_name.ends_with(close) {
            continue;
        }
        let open_index = display_name.rfind(open)?;
        let campus_name = display_name
            [open_index + open.len_utf8()..display_name.len() - close.len_utf8()]
            .trim();
        let school_name = display_name[..open_index].trim();
        if !school_name.is_empty() && is_campus_label(campus_name) {
            return Some((school_name.to_owned(), campus_name.to_owned()));
        }
    }
    None
}

fn trailing_campus_separator(display_name: &str) -> Option<(String, String)> {
    for separator in ['-', '－', '—'] {
        if let Some((school_name, campus_name)) = display_name.rsplit_once(separator) {
            let school_name = school_name.trim();
            let campus_name = campus_name.trim();
            if !school_name.is_empty() && is_campus_label(campus_name) {
                return Some((school_name.to_owned(), campus_name.to_owned()));
            }
        }
    }
    None
}

fn is_campus_label(value: &str) -> bool {
    value.contains("校区") || value.contains("校园")
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("AMAP_WEB_SERVICE_KEY is not configured")]
    MissingApiKey,
    #[error("campus search keyword cannot be blank")]
    BlankKeyword,
    #[error("campus search network request failed")]
    Transport,
    #[error("campus search HTTP request failed with status {0}")]
    HttpStatus(u16),
    #[error("campus search service rejected the request: {info} ({infocode})")]
    ServiceRejected { info: String, infocode: String },
    #[error("campus search returned malformed data: {0}")]
    MalformedResponse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_with(pois: &str) -> String {
        format!(r#"{{"status":"1","info":"OK","infocode":"10000","pois":[{pois}]}}"#)
    }

    #[test]
    fn request_restricts_search_to_higher_education() {
        let url = build_search_url("secret123", "华东师范大学", Some("上海市")).unwrap();
        assert!(url.contains("types=141201"));
        assert!(url.contains("city=%E4%B8%8A%E6%B5%B7%E5%B8%82"));
        assert!(url.contains("keywords=%E5%8D%8E%E4%B8%9C%E5%B8%88%E8%8C%83%E5%A4%A7%E5%AD%A6"));
    }

    #[test]
    fn blank_keyword_is_rejected_before_request() {
        assert!(matches!(
            build_search_url("secret123", "   ", None),
            Err(SearchError::BlankKeyword)
        ));
    }

    #[test]
    fn parser_keeps_only_higher_education_and_skips_bad_coordinates() {
        let json = response_with(
            r#"{"id":"B01","name":"华东师范大学(普陀校区)","typecode":"141201","address":"中山北路3663号","location":"121.406,31.228","pname":"上海市","cityname":"上海市","adname":"普陀区"},
               {"id":"B02","name":"某中学","typecode":"141202","address":"人民路1号","location":"121.4,31.2"},
               {"id":"B03","name":"某大学","typecode":"141201","address":"大学路1号","location":"999,999"}"#,
        );

        let candidates = parse_search_response(&json).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].identity.school_name, "华东师范大学");
        assert_eq!(
            candidates[0].identity.campus_name.as_deref(),
            Some("普陀校区")
        );
        assert_eq!(candidates[0].source.external_id, "B01");
        assert!((candidates[0].anchor.longitude() - 121.406).abs() > 0.001);
        assert!((candidates[0].anchor.latitude() - 31.228).abs() > 0.001);
    }

    #[test]
    fn gcj02_inverse_round_trip_is_stable_for_shanghai() {
        let gcj = (121.406, 31.228);
        let wgs = gcj02_to_wgs84(gcj.0, gcj.1);
        let reconstructed_gcj = wgs84_to_gcj02(wgs.0, wgs.1);

        assert!((reconstructed_gcj.0 - gcj.0).abs() < 1e-7);
        assert!((reconstructed_gcj.1 - gcj.1).abs() < 1e-7);
    }

    #[test]
    fn gcj02_inverse_is_identity_outside_china() {
        assert_eq!(gcj02_to_wgs84(-74.006, 40.7128), (-74.006, 40.7128));
    }

    #[test]
    fn duplicate_poi_ids_are_returned_once() {
        let json = response_with(
            r#"{"id":"B01","name":"某大学(主校区)","typecode":"141201","address":"大学路1号","location":"121.4,31.2"},
               {"id":"B01","name":"某大学(主校区)","typecode":"141201","address":"大学路1号","location":"121.4,31.2"}"#,
        );
        assert_eq!(parse_search_response(&json).unwrap().len(), 1);
    }

    #[test]
    fn exact_display_name_is_preserved_while_extracting_campus_label() {
        let identity = parse_campus_identity("上海交通大学(闵行本部校区)");
        assert_eq!(identity.school_name, "上海交通大学");
        assert_eq!(identity.campus_name.as_deref(), Some("闵行本部校区"));
        assert_eq!(identity.display_name, "上海交通大学(闵行本部校区)");

        let identity = parse_campus_identity("复旦大学-邯郸校区");
        assert_eq!(identity.school_name, "复旦大学");
        assert_eq!(identity.campus_name.as_deref(), Some("邯郸校区"));
    }

    #[test]
    fn campus_name_is_not_invented_when_provider_has_no_explicit_label() {
        let identity = parse_campus_identity("同济大学");
        assert_eq!(identity.school_name, "同济大学");
        assert_eq!(identity.campus_name, None);
        assert_eq!(identity.display_name, "同济大学");
    }

    #[test]
    fn service_error_is_structured_without_request_url_or_key() {
        let error = parse_search_response(
            r#"{"status":"0","info":"INVALID_USER_KEY","infocode":"10001","pois":[]}"#,
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("INVALID_USER_KEY"));
        assert!(!message.contains("secret123"));
        assert!(!message.contains("restapi.amap.com"));
    }
}
