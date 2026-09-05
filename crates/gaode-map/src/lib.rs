use campus_core::{CampusBoundary, Wgs84Coordinate};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use thiserror::Error;

const KRASOVSKY_A: f64 = 6_378_245.0;
const KRASOVSKY_EE: f64 = 0.006_693_421_622_965_943;
const GCJ_INVERSE_ITERATIONS: usize = 4;

#[derive(Debug, Clone)]
pub struct BoundaryMapConfig {
    pub js_api_key: String,
    pub security_code: String,
    pub campus_display_name: String,
    pub anchor: Wgs84Coordinate,
    pub existing_boundary: Option<CampusBoundary>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BoundaryMapEvent {
    Ready,
    Cancel,
    SubmitBoundary(Vec<Wgs84Coordinate>),
}

#[derive(Debug, Error)]
pub enum BoundaryMapError {
    #[error("AMAP_JS_KEY is not configured")]
    MissingJsApiKey,
    #[error("AMAP_JS_SECURITY_CODE is not configured")]
    MissingSecurityCode,
    #[error("boundary map message is invalid: {0}")]
    InvalidMessage(String),
    #[error("boundary map coordinate at index {index} is invalid")]
    InvalidCoordinate { index: usize },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BoundaryBootstrap {
    security_code: String,
    campus_display_name: String,
    anchor_gcj02: [f64; 2],
    existing_boundary_gcj02: Option<Vec<[f64; 2]>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawBoundaryMapEvent {
    Ready,
    Cancel,
    SubmitBoundary { coordinates: Vec<[f64; 2]> },
}

pub fn build_boundary_editor_html(config: &BoundaryMapConfig) -> Result<String, BoundaryMapError> {
    let js_api_key = config.js_api_key.trim();
    if js_api_key.is_empty() {
        return Err(BoundaryMapError::MissingJsApiKey);
    }
    if config.security_code.trim().is_empty() {
        return Err(BoundaryMapError::MissingSecurityCode);
    }

    let (anchor_lon, anchor_lat) = wgs84_to_gcj02(config.anchor);
    let existing_boundary_gcj02 = config.existing_boundary.as_ref().map(|boundary| {
        boundary
            .vertices()
            .iter()
            .copied()
            .map(wgs84_to_gcj02)
            .map(|(longitude, latitude)| [longitude, latitude])
            .collect()
    });
    let bootstrap = BoundaryBootstrap {
        security_code: config.security_code.clone(),
        campus_display_name: config.campus_display_name.clone(),
        anchor_gcj02: [anchor_lon, anchor_lat],
        existing_boundary_gcj02,
    };
    let json = serde_json::to_string(&bootstrap)
        .map_err(|error| BoundaryMapError::InvalidMessage(error.to_string()))?;
    let safe_json = json
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");

    Ok(BOUNDARY_EDITOR_HTML
        .replace("__BOOTSTRAP_JSON__", &safe_json)
        .replace("__AMAP_JS_KEY__", urlencoding::encode(js_api_key).as_ref()))
}

pub fn parse_boundary_map_event(message: &str) -> Result<BoundaryMapEvent, BoundaryMapError> {
    let raw: RawBoundaryMapEvent = serde_json::from_str(message)
        .map_err(|error| BoundaryMapError::InvalidMessage(error.to_string()))?;
    match raw {
        RawBoundaryMapEvent::Ready => Ok(BoundaryMapEvent::Ready),
        RawBoundaryMapEvent::Cancel => Ok(BoundaryMapEvent::Cancel),
        RawBoundaryMapEvent::SubmitBoundary { coordinates } => {
            let mut vertices = Vec::with_capacity(coordinates.len());
            for (index, [longitude, latitude]) in coordinates.into_iter().enumerate() {
                if !is_valid_pair(longitude, latitude) {
                    return Err(BoundaryMapError::InvalidCoordinate { index });
                }
                let (wgs_lon, wgs_lat) = gcj02_to_wgs84(longitude, latitude);
                let coordinate = Wgs84Coordinate::try_new(wgs_lon, wgs_lat)
                    .map_err(|_| BoundaryMapError::InvalidCoordinate { index })?;
                vertices.push(coordinate);
            }
            Ok(BoundaryMapEvent::SubmitBoundary(vertices))
        }
    }
}

fn is_valid_pair(longitude: f64, latitude: f64) -> bool {
    longitude.is_finite()
        && latitude.is_finite()
        && (-180.0..=180.0).contains(&longitude)
        && (-90.0..=90.0).contains(&latitude)
}

fn wgs84_to_gcj02(coordinate: Wgs84Coordinate) -> (f64, f64) {
    wgs84_pair_to_gcj02(coordinate.longitude(), coordinate.latitude())
}

fn wgs84_pair_to_gcj02(longitude: f64, latitude: f64) -> (f64, f64) {
    if out_of_china(longitude, latitude) {
        return (longitude, latitude);
    }
    let d_lat = transform_latitude(longitude - 105.0, latitude - 35.0);
    let d_lon = transform_longitude(longitude - 105.0, latitude - 35.0);
    let rad_lat = latitude.to_radians();
    let sin_lat = rad_lat.sin();
    let magic = 1.0 - KRASOVSKY_EE * sin_lat * sin_lat;
    let sqrt_magic = magic.sqrt();
    let d_lat = (d_lat * 180.0)
        / ((KRASOVSKY_A * (1.0 - KRASOVSKY_EE)) / (magic * sqrt_magic) * PI);
    let d_lon = (d_lon * 180.0) / (KRASOVSKY_A / sqrt_magic * rad_lat.cos() * PI);
    (longitude + d_lon, latitude + d_lat)
}

fn gcj02_to_wgs84(gcj_longitude: f64, gcj_latitude: f64) -> (f64, f64) {
    if out_of_china(gcj_longitude, gcj_latitude) {
        return (gcj_longitude, gcj_latitude);
    }
    let mut wgs_lon = gcj_longitude;
    let mut wgs_lat = gcj_latitude;
    for _ in 0..GCJ_INVERSE_ITERATIONS {
        let (estimated_lon, estimated_lat) = wgs84_pair_to_gcj02(wgs_lon, wgs_lat);
        wgs_lon -= estimated_lon - gcj_longitude;
        wgs_lat -= estimated_lat - gcj_latitude;
    }
    (wgs_lon, wgs_lat)
}

fn out_of_china(longitude: f64, latitude: f64) -> bool {
    !(72.004..=137.8347).contains(&longitude) || !(0.8293..=55.8271).contains(&latitude)
}

fn transform_latitude(x: f64, y: f64) -> f64 {
    let mut result = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * x.abs().sqrt();
    result += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    result += (20.0 * (y * PI).sin() + 40.0 * (y / 3.0 * PI).sin()) * 2.0 / 3.0;
    result += (160.0 * (y / 12.0 * PI).sin() + 320.0 * (y * PI / 30.0).sin()) * 2.0 / 3.0;
    result
}

fn transform_longitude(x: f64, y: f64) -> f64 {
    let mut result = 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * x.abs().sqrt();
    result += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    result += (20.0 * (x * PI).sin() + 40.0 * (x / 3.0 * PI).sin()) * 2.0 / 3.0;
    result += (150.0 * (x / 12.0 * PI).sin() + 300.0 * (x / 30.0 * PI).sin()) * 2.0 / 3.0;
    result
}

const BOUNDARY_EDITOR_HTML: &str = r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>MCRebuild 边界编辑</title>
<style>
html,body{height:100%;margin:0;font-family:system-ui,-apple-system,"Segoe UI",sans-serif;background:#f5f6f8;color:#1f2328}
#app{height:100%;display:grid;grid-template-columns:280px 1fr}#panel{box-sizing:border-box;padding:22px;background:#fff;border-right:1px solid #e5e7eb;display:flex;flex-direction:column;gap:16px}
#campus{font-size:18px;font-weight:650;line-height:1.35}#hint,#status{font-size:13px;line-height:1.55;color:#59636e}#status{padding:10px 12px;border-radius:8px;background:#f3f4f6}
.actions{display:grid;gap:10px;margin-top:auto}button{border:1px solid #cfd5dc;border-radius:8px;padding:10px 12px;background:#fff;font-size:14px;cursor:pointer}button.primary{border-color:#2563eb;background:#2563eb;color:#fff}button:disabled{opacity:.45;cursor:not-allowed}#map{height:100%;min-width:0}
</style>
<script id="mcrebuild-bootstrap" type="application/json">__BOOTSTRAP_JSON__</script>
<script>const bootstrap=JSON.parse(document.getElementById('mcrebuild-bootstrap').textContent);window._AMapSecurityConfig={securityJsCode:bootstrap.securityCode};</script>
<script src="https://webapi.amap.com/maps?v=2.0&key=__AMAP_JS_KEY__&plugin=AMap.MouseTool,AMap.PolygonEditor"></script></head>
<body><div id="app"><aside id="panel"><div><div id="campus"></div><div id="hint">拖动顶点可修正边界；需要重新圈选时点击“重新圈画”。边界有效性由 MCRebuild Core 最终校验。</div></div><div id="status">正在加载地图…</div><div class="actions"><button id="redraw">重新圈画</button><button id="save" class="primary" disabled>保存边界</button><button id="cancel">关闭</button></div></aside><main id="map"></main></div>
<script>(function(){
const campus=document.getElementById('campus'),status=document.getElementById('status'),save=document.getElementById('save'),redraw=document.getElementById('redraw'),cancel=document.getElementById('cancel');campus.textContent=bootstrap.campusDisplayName;
function send(message){if(window.ipc&&window.ipc.postMessage){window.ipc.postMessage(JSON.stringify(message));}}function setStatus(text){status.textContent=text;}
const map=new AMap.Map('map',{center:bootstrap.anchorGcj02,zoom:16,viewMode:'2D'});const mouseTool=new AMap.MouseTool(map);let polygon=null,editor=null;
function closeEditor(){if(editor){editor.close();editor=null;}}function removePolygon(){closeEditor();if(polygon){map.remove(polygon);polygon=null;}save.disabled=true;}
function editPolygon(next){closeEditor();polygon=next;editor=new AMap.PolygonEditor(map,polygon);editor.open();save.disabled=false;map.setFitView([polygon],false,[80,80,80,80]);setStatus('边界可编辑；确认无误后保存。');}
function startDrawing(){removePolygon();mouseTool.polygon({strokeColor:'#2563eb',strokeWeight:3,fillColor:'#2563eb',fillOpacity:.14});setStatus('单击添加顶点，双击结束圈画。');}
mouseTool.on('draw',function(event){mouseTool.close(false);editPolygon(event.obj);});
if(bootstrap.existingBoundaryGcj02&&bootstrap.existingBoundaryGcj02.length>=3){editPolygon(new AMap.Polygon({path:bootstrap.existingBoundaryGcj02,strokeColor:'#2563eb',strokeWeight:3,fillColor:'#2563eb',fillOpacity:.14}));}else{startDrawing();}
redraw.addEventListener('click',startDrawing);save.addEventListener('click',function(){if(!polygon)return;const coordinates=polygon.getPath().map(function(p){return[p.lng,p.lat];});setStatus('正在由 MCRebuild 校验并保存…');save.disabled=true;send({type:'submit_boundary',coordinates});});cancel.addEventListener('click',function(){send({type:'cancel'});});
window.mcrebuildBoundaryResult=function(result){save.disabled=!polygon;setStatus(result&&result.message?result.message:'边界处理完成。');};map.on('complete',function(){send({type:'ready'});});
})();</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn point(longitude: f64, latitude: f64) -> Wgs84Coordinate {
        Wgs84Coordinate::try_new(longitude, latitude).unwrap()
    }

    fn config() -> BoundaryMapConfig {
        BoundaryMapConfig {
            js_api_key: "abc+123".into(),
            security_code: "security-code".into(),
            campus_display_name: "华东师范大学(普陀校区)".into(),
            anchor: point(121.400, 31.226),
            existing_boundary: None,
        }
    }

    #[test]
    fn generated_page_uses_encoded_key_and_small_ipc_contract() {
        let html = build_boundary_editor_html(&config()).unwrap();
        assert!(html.contains("key=abc%2B123"));
        assert!(html.contains("submit_boundary"));
        assert!(html.contains("AMap.PolygonEditor"));
        assert!(!html.contains("AMap.PlaceSearch"));
    }

    #[test]
    fn bootstrap_json_cannot_break_out_of_script_element() {
        let mut value = config();
        value.campus_display_name = "</script><script>alert(1)</script>".into();
        let html = build_boundary_editor_html(&value).unwrap();
        assert!(!html.contains("</script><script>alert(1)</script>"));
        assert!(html.contains("\\u003c/script\\u003e"));
    }

    #[test]
    fn missing_web_keys_are_rejected() {
        let mut value = config();
        value.js_api_key.clear();
        assert!(matches!(
            build_boundary_editor_html(&value),
            Err(BoundaryMapError::MissingJsApiKey)
        ));
        let mut value = config();
        value.security_code.clear();
        assert!(matches!(
            build_boundary_editor_html(&value),
            Err(BoundaryMapError::MissingSecurityCode)
        ));
    }

    #[test]
    fn gcj_submission_is_normalized_back_to_wgs84() {
        let original = point(121.400, 31.226);
        let (gcj_lon, gcj_lat) = wgs84_to_gcj02(original);
        let message = serde_json::json!({
            "type": "submit_boundary",
            "coordinates": [
                [gcj_lon, gcj_lat],
                [gcj_lon + 0.001, gcj_lat],
                [gcj_lon + 0.001, gcj_lat + 0.001]
            ]
        })
        .to_string();
        let event = parse_boundary_map_event(&message).unwrap();
        let BoundaryMapEvent::SubmitBoundary(vertices) = event else {
            panic!("expected boundary submission");
        };
        assert!((vertices[0].longitude() - original.longitude()).abs() < 1e-7);
        assert!((vertices[0].latitude() - original.latitude()).abs() < 1e-7);
    }

    #[test]
    fn transport_messages_parse_without_project_logic() {
        assert_eq!(
            parse_boundary_map_event(r#"{"type":"ready"}"#).unwrap(),
            BoundaryMapEvent::Ready
        );
        assert_eq!(
            parse_boundary_map_event(r#"{"type":"cancel"}"#).unwrap(),
            BoundaryMapEvent::Cancel
        );
    }

    #[test]
    fn out_of_range_coordinates_are_rejected() {
        let error = parse_boundary_map_event(
            r#"{"type":"submit_boundary","coordinates":[[999.0,31.2]]}"#,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            BoundaryMapError::InvalidCoordinate { index: 0 }
        ));
    }
}
