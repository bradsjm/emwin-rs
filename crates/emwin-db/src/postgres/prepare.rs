use super::{IncidentChangeAction, IncidentKey, PersistError, PersistResult};
use crate::metadata::CompletedFileMetadata;
use crate::writer::{BlobRole, StoredBlob};
use emwin_parser::{
    GenericBody, HvtecCode, ProductBody, ProductHeaderV2, TimeMotLocEntry, UgcArea, UgcSection,
    VtecCode, VtecEventBody,
};
use emwin_service::SourceKind;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug)]
pub(super) struct PreparedProduct {
    pub(super) row: ProductRow,
    pub(super) issues: Vec<ProductIssueRow>,
    pub(super) vtec: Vec<ProductVtecRow>,
    pub(super) incident_updates: Vec<PreparedIncidentUpdate>,
    pub(super) ugc_areas: Vec<ProductUgcAreaRow>,
    pub(super) hvtec: Vec<ProductHvtecRow>,
    pub(super) time_mot_loc: Vec<ProductTimeMotLocRow>,
    pub(super) polygons: Vec<ProductPolygonRow>,
    pub(super) wind_hail: Vec<ProductWindHailRow>,
    pub(super) search_points: Vec<ProductSearchPointRow>,
}

#[derive(Debug)]
pub(super) struct ProductRow {
    pub(super) filename: String,
    pub(super) source_timestamp_utc: i64,
    pub(super) source_receiver: String,
    pub(super) source_message_id: Option<String>,
    pub(super) size_bytes: i64,
    pub(super) payload_location: String,
    pub(super) metadata_location: Option<String>,
    pub(super) source: String,
    pub(super) family: Option<String>,
    pub(super) artifact_kind: Option<String>,
    pub(super) title: Option<String>,
    pub(super) container: String,
    pub(super) pil: Option<String>,
    pub(super) wmo_prefix: Option<String>,
    pub(super) bbb_kind: Option<String>,
    pub(super) office_code: Option<String>,
    pub(super) office_city: Option<String>,
    pub(super) office_state: Option<String>,
    pub(super) header_kind: Option<String>,
    pub(super) ttaaii: Option<String>,
    pub(super) cccc: Option<String>,
    pub(super) ddhhmm: Option<String>,
    pub(super) bbb: Option<String>,
    pub(super) afos: Option<String>,
    pub(super) has_body: bool,
    pub(super) has_artifact: bool,
    pub(super) has_issues: bool,
    pub(super) has_vtec: bool,
    pub(super) has_ugc: bool,
    pub(super) has_hvtec: bool,
    pub(super) has_latlon: bool,
    pub(super) has_time_mot_loc: bool,
    pub(super) has_wind_hail: bool,
    pub(super) vtec_count: i32,
    pub(super) ugc_count: i32,
    pub(super) hvtec_count: i32,
    pub(super) latlon_count: i32,
    pub(super) time_mot_loc_count: i32,
    pub(super) wind_hail_count: i32,
    pub(super) issue_count: i32,
    pub(super) states: Vec<String>,
    pub(super) ugc_codes: Vec<String>,
    pub(super) product_json: Value,
}

#[derive(Debug)]
pub(super) struct ProductIssueRow {
    pub(super) kind: String,
    pub(super) code: String,
    pub(super) message: String,
    pub(super) line: Option<String>,
}

#[derive(Debug)]
pub(super) struct ProductVtecRow {
    pub(super) segment_index: Option<i32>,
    pub(super) status: String,
    pub(super) action: String,
    pub(super) office: String,
    pub(super) phenomena: String,
    pub(super) significance: String,
    pub(super) etn: i64,
    pub(super) begin_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(super) end_utc: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug)]
pub(super) struct PreparedIncidentUpdate {
    pub(super) key: IncidentKey,
    pub(super) current_status: Option<String>,
    pub(super) latest_vtec_action: String,
    pub(super) issued_at: chrono::DateTime<chrono::Utc>,
    pub(super) start_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(super) end_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(super) latest_product_timestamp_utc: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub(super) struct PendingIncidentChange {
    pub(super) key: IncidentKey,
    pub(super) action: IncidentChangeAction,
}

#[derive(Debug)]
pub(super) struct ProductUgcAreaRow {
    pub(super) segment_index: Option<i32>,
    pub(super) section_index: i32,
    pub(super) area_kind: String,
    pub(super) state: String,
    pub(super) ugc_code: String,
    pub(super) name: Option<String>,
    pub(super) expires_utc: chrono::DateTime<chrono::Utc>,
    pub(super) latitude: Option<f64>,
    pub(super) longitude: Option<f64>,
}

#[derive(Debug)]
pub(super) struct ProductHvtecRow {
    pub(super) segment_index: Option<i32>,
    pub(super) hvtec_index: i32,
    pub(super) nwslid: String,
    pub(super) location_name: Option<String>,
    pub(super) severity: String,
    pub(super) cause: String,
    pub(super) record: String,
    pub(super) begin_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(super) crest_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(super) end_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(super) latitude: Option<f64>,
    pub(super) longitude: Option<f64>,
}

#[derive(Debug)]
pub(super) struct ProductTimeMotLocRow {
    pub(super) segment_index: Option<i32>,
    pub(super) entry_index: i32,
    pub(super) time_utc: chrono::DateTime<chrono::Utc>,
    pub(super) direction_degrees: i32,
    pub(super) speed_kt: i32,
    pub(super) path_wkt: String,
}

#[derive(Debug)]
pub(super) struct ProductPolygonRow {
    pub(super) segment_index: Option<i32>,
    pub(super) polygon_index: i32,
    pub(super) polygon_wkt: String,
}

#[derive(Debug)]
pub(super) struct ProductWindHailRow {
    pub(super) segment_index: Option<i32>,
    pub(super) entry_index: i32,
    pub(super) kind: String,
    pub(super) numeric_value: Option<f64>,
    pub(super) units: Option<String>,
    pub(super) comparison: Option<String>,
}

#[derive(Debug)]
pub(super) struct ProductSearchPointRow {
    pub(super) source_kind: String,
    pub(super) source_index: i32,
    pub(super) latitude: f64,
    pub(super) longitude: f64,
}

#[derive(Debug)]
pub(super) struct UgcBucketSpec<'a> {
    pub(super) area_kind: &'a str,
    pub(super) bucket: &'a BTreeMap<String, Vec<UgcArea>>,
    pub(super) code_prefix: char,
}

#[derive(Debug)]
pub(super) struct HeaderColumns {
    pub(super) header_kind: Option<String>,
    pub(super) ttaaii: Option<String>,
    pub(super) cccc: Option<String>,
    pub(super) ddhhmm: Option<String>,
    pub(super) bbb: Option<String>,
    pub(super) afos: Option<String>,
}

impl PreparedProduct {
    pub(super) fn prepare(
        metadata: &CompletedFileMetadata,
        blobs: &[StoredBlob],
    ) -> PersistResult<Self> {
        let payload = find_blob(blobs, BlobRole::Payload)?;
        let sidecar = find_blob_optional(blobs, BlobRole::MetadataSidecar);
        let product_summary = metadata.product_summary();
        let product_detail = metadata.product_detail();
        let header = product_summary.header.as_ref();
        let HeaderColumns {
            header_kind,
            ttaaii,
            cccc,
            ddhhmm,
            bbb,
            afos,
        } = flatten_header(header);

        let row = ProductRow {
            filename: metadata.filename.clone(),
            source_timestamp_utc: i64::try_from(metadata.timestamp_utc).map_err(|_| {
                PersistError::InvalidRequest(format!(
                    "timestamp `{}` does not fit in bigint",
                    metadata.timestamp_utc
                ))
            })?,
            source_receiver: source_receiver(&metadata.origin).to_string(),
            source_message_id: source_message_id(&metadata.origin),
            size_bytes: i64::try_from(metadata.size).map_err(|_| {
                PersistError::InvalidRequest(format!(
                    "size `{}` does not fit in bigint",
                    metadata.size
                ))
            })?,
            payload_location: payload.location.clone(),
            metadata_location: sidecar.map(|blob| blob.location.clone()),
            source: serde_label(&product_summary.source)?,
            family: product_summary.family.map(str::to_string),
            artifact_kind: product_summary.artifact_kind.map(str::to_string),
            title: product_summary.title.map(str::to_string),
            container: product_summary.container.to_string(),
            pil: product_summary.pil.clone(),
            wmo_prefix: product_summary.wmo_prefix.map(str::to_string),
            bbb_kind: product_summary
                .bbb_kind
                .as_ref()
                .map(serde_label)
                .transpose()?,
            office_code: product_summary
                .office
                .as_ref()
                .map(|office| office.code.to_string()),
            office_city: product_summary
                .office
                .as_ref()
                .map(|office| office.city.to_string()),
            office_state: product_summary
                .office
                .as_ref()
                .map(|office| office.state.to_string()),
            header_kind,
            ttaaii,
            cccc,
            ddhhmm,
            bbb,
            afos,
            has_body: product_summary.facets.has_body,
            has_artifact: product_summary.facets.has_artifact,
            has_issues: product_summary.facets.has_issues,
            has_vtec: product_summary.facets.vtec_count > 0,
            has_ugc: product_summary.facets.ugc_count > 0,
            has_hvtec: product_summary.facets.hvtec_count > 0,
            has_latlon: product_summary.facets.latlon_count > 0,
            has_time_mot_loc: product_summary.facets.time_mot_loc_count > 0,
            has_wind_hail: product_summary.facets.wind_hail_count > 0,
            vtec_count: usize_to_i32(product_summary.facets.vtec_count, "vtec_count")?,
            ugc_count: usize_to_i32(product_summary.facets.ugc_count, "ugc_count")?,
            hvtec_count: usize_to_i32(product_summary.facets.hvtec_count, "hvtec_count")?,
            latlon_count: usize_to_i32(product_summary.facets.latlon_count, "latlon_count")?,
            time_mot_loc_count: usize_to_i32(
                product_summary.facets.time_mot_loc_count,
                "time_mot_loc_count",
            )?,
            wind_hail_count: usize_to_i32(
                product_summary.facets.wind_hail_count,
                "wind_hail_count",
            )?,
            issue_count: usize_to_i32(product_summary.issues.count, "issue_count")?,
            states: product_summary.keys.states.clone(),
            ugc_codes: product_summary.keys.ugc_codes.clone(),
            product_json: serde_json::to_value(&product_detail)?,
        };

        let issues = metadata
            .product
            .issues
            .iter()
            .map(|issue| ProductIssueRow {
                kind: issue.kind.to_string(),
                code: issue.code.to_string(),
                message: issue.message.clone(),
                line: issue.line.clone(),
            })
            .collect();

        let mut prepared = Self {
            row,
            issues,
            vtec: Vec::new(),
            incident_updates: Vec::new(),
            ugc_areas: Vec::new(),
            hvtec: Vec::new(),
            time_mot_loc: Vec::new(),
            polygons: Vec::new(),
            wind_hail: Vec::new(),
            search_points: Vec::new(),
        };

        if let Some(body) = metadata.product.body.as_ref() {
            collect_body_rows(&mut prepared, body)?;
        }

        prepared.incident_updates =
            prepare_incident_updates(prepared.row.source_timestamp_utc, &prepared.vtec)?;

        Ok(prepared)
    }
}

fn prepare_incident_updates(
    source_timestamp_utc: i64,
    vtec_rows: &[ProductVtecRow],
) -> PersistResult<Vec<PreparedIncidentUpdate>> {
    #[derive(Debug)]
    struct IncidentAccumulator {
        current_status: Option<String>,
        latest_vtec_action: String,
        start_utc: Option<chrono::DateTime<chrono::Utc>>,
        end_utc: Option<chrono::DateTime<chrono::Utc>>,
    }

    let issued_at = chrono::DateTime::from_timestamp(source_timestamp_utc, 0).ok_or_else(|| {
        PersistError::InvalidRequest(format!(
            "timestamp `{source_timestamp_utc}` cannot convert to timestamptz"
        ))
    })?;

    let mut grouped = BTreeMap::<IncidentKey, IncidentAccumulator>::new();
    for vtec in vtec_rows.iter().filter(|vtec| vtec.status == "O") {
        let key = IncidentKey {
            office: vtec.office.clone(),
            phenomena: vtec.phenomena.clone(),
            significance: vtec.significance.clone(),
            etn: vtec.etn,
        };
        let entry = grouped.entry(key).or_insert_with(|| IncidentAccumulator {
            current_status: map_incident_status(&vtec.action),
            latest_vtec_action: vtec.action.clone(),
            start_utc: vtec.begin_utc,
            end_utc: vtec.end_utc,
        });

        if let Some(current_status) = map_incident_status(&vtec.action) {
            entry.current_status = Some(current_status);
        }
        entry.latest_vtec_action = vtec.action.clone();
        entry.start_utc = min_option_datetime(entry.start_utc, vtec.begin_utc);
        entry.end_utc = max_option_datetime(entry.end_utc, vtec.end_utc);
    }

    Ok(grouped
        .into_iter()
        .map(|(key, value)| PreparedIncidentUpdate {
            key,
            current_status: value.current_status,
            latest_vtec_action: value.latest_vtec_action,
            issued_at,
            start_utc: value.start_utc,
            end_utc: value.end_utc,
            latest_product_timestamp_utc: issued_at,
        })
        .collect())
}

fn map_incident_status(action: &str) -> Option<String> {
    match action {
        "NEW" | "CON" | "EXT" | "EXA" | "EXB" => Some("active".to_string()),
        "CAN" => Some("cancelled".to_string()),
        "EXP" => Some("expired".to_string()),
        "UPG" => Some("upgraded".to_string()),
        "COR" | "ROU" => None,
        _ => None,
    }
}

fn min_option_datetime(
    current: Option<chrono::DateTime<chrono::Utc>>,
    incoming: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    match (current, incoming) {
        (Some(current), Some(incoming)) => Some(current.min(incoming)),
        (Some(current), None) => Some(current),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn max_option_datetime(
    current: Option<chrono::DateTime<chrono::Utc>>,
    incoming: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    match (current, incoming) {
        (Some(current), Some(incoming)) => Some(current.max(incoming)),
        (Some(current), None) => Some(current),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn collect_body_rows(prepared: &mut PreparedProduct, body: &ProductBody) -> PersistResult<()> {
    match body {
        ProductBody::VtecEvent(body) => collect_vtec_event_rows(prepared, body)?,
        ProductBody::Generic(body) => collect_generic_rows(prepared, None, body)?,
    }
    Ok(())
}

fn collect_vtec_event_rows(
    prepared: &mut PreparedProduct,
    body: &VtecEventBody,
) -> PersistResult<()> {
    for segment in &body.segments {
        let segment_index = Some(usize_to_i32(segment.segment_index, "segment_index")?);
        for vtec in &segment.vtec {
            push_vtec_row(&mut prepared.vtec, segment_index, vtec);
        }
        for (hvtec_index, hvtec) in segment.hvtec.iter().enumerate() {
            push_hvtec_row(
                &mut prepared.hvtec,
                &mut prepared.search_points,
                segment_index,
                usize_to_i32(hvtec_index, "hvtec_index")?,
                hvtec,
            )?;
        }
        for (polygon_index, polygon) in segment.polygons.iter().enumerate() {
            prepared.polygons.push(ProductPolygonRow {
                segment_index,
                polygon_index: usize_to_i32(polygon_index, "polygon_index")?,
                polygon_wkt: polygon.wkt.clone(),
            });
        }
        for (entry_index, entry) in segment.time_mot_loc.iter().enumerate() {
            push_time_mot_loc_row(
                &mut prepared.time_mot_loc,
                &mut prepared.search_points,
                segment_index,
                usize_to_i32(entry_index, "time_mot_loc_index")?,
                entry,
            )?;
        }
        for (entry_index, entry) in segment.wind_hail.iter().enumerate() {
            prepared.wind_hail.push(ProductWindHailRow {
                segment_index,
                entry_index: usize_to_i32(entry_index, "wind_hail_index")?,
                kind: serde_label(&entry.kind)?,
                numeric_value: entry.numeric_value,
                units: entry.units.clone(),
                comparison: entry.comparison.map(|value| value.to_string()),
            });
        }
        collect_ugc_rows(
            &mut prepared.ugc_areas,
            &mut prepared.search_points,
            segment_index,
            &segment.ugc_sections,
        )?;
    }
    Ok(())
}

fn collect_generic_rows(
    prepared: &mut PreparedProduct,
    segment_index: Option<i32>,
    body: &GenericBody,
) -> PersistResult<()> {
    if let Some(sections) = body.ugc.as_ref() {
        collect_ugc_rows(
            &mut prepared.ugc_areas,
            &mut prepared.search_points,
            segment_index,
            sections,
        )?;
    }
    if let Some(polygons) = body.latlon.as_ref() {
        for (polygon_index, polygon) in polygons.iter().enumerate() {
            prepared.polygons.push(ProductPolygonRow {
                segment_index,
                polygon_index: usize_to_i32(polygon_index, "polygon_index")?,
                polygon_wkt: polygon.wkt.clone(),
            });
        }
    }
    if let Some(entries) = body.time_mot_loc.as_ref() {
        for (entry_index, entry) in entries.iter().enumerate() {
            push_time_mot_loc_row(
                &mut prepared.time_mot_loc,
                &mut prepared.search_points,
                segment_index,
                usize_to_i32(entry_index, "time_mot_loc_index")?,
                entry,
            )?;
        }
    }
    if let Some(entries) = body.wind_hail.as_ref() {
        for (entry_index, entry) in entries.iter().enumerate() {
            prepared.wind_hail.push(ProductWindHailRow {
                segment_index,
                entry_index: usize_to_i32(entry_index, "wind_hail_index")?,
                kind: serde_label(&entry.kind)?,
                numeric_value: entry.numeric_value,
                units: entry.units.clone(),
                comparison: entry.comparison.map(|value| value.to_string()),
            });
        }
    }
    Ok(())
}

fn collect_ugc_rows(
    target: &mut Vec<ProductUgcAreaRow>,
    search_points: &mut Vec<ProductSearchPointRow>,
    segment_index: Option<i32>,
    sections: &[UgcSection],
) -> PersistResult<()> {
    for (section_index, section) in sections.iter().enumerate() {
        let section_index = usize_to_i32(section_index, "ugc_section_index")?;
        for spec in [
            UgcBucketSpec {
                area_kind: "county",
                bucket: &section.counties,
                code_prefix: 'C',
            },
            UgcBucketSpec {
                area_kind: "zone",
                bucket: &section.zones,
                code_prefix: 'Z',
            },
            UgcBucketSpec {
                area_kind: "fire_zone",
                bucket: &section.fire_zones,
                code_prefix: 'F',
            },
            UgcBucketSpec {
                area_kind: "marine_zone",
                bucket: &section.marine_zones,
                code_prefix: 'M',
            },
        ] {
            push_ugc_bucket(
                target,
                search_points,
                segment_index,
                section_index,
                section,
                spec,
            );
        }
    }
    Ok(())
}

fn push_ugc_bucket(
    target: &mut Vec<ProductUgcAreaRow>,
    search_points: &mut Vec<ProductSearchPointRow>,
    segment_index: Option<i32>,
    section_index: i32,
    section: &UgcSection,
    spec: UgcBucketSpec<'_>,
) {
    for (state, areas) in spec.bucket {
        for area in areas {
            let ugc_code = format!("{state}{}{:03}", spec.code_prefix, area.id);
            target.push(ProductUgcAreaRow {
                segment_index,
                section_index,
                area_kind: spec.area_kind.to_string(),
                state: state.clone(),
                ugc_code,
                name: area.name.map(str::to_string),
                expires_utc: section.expires,
                latitude: area.lat,
                longitude: area.lon,
            });
            if let (Some(latitude), Some(longitude)) = (area.lat, area.lon) {
                search_points.push(ProductSearchPointRow {
                    source_kind: spec.area_kind.to_string(),
                    source_index: section_index,
                    latitude,
                    longitude,
                });
            }
        }
    }
}

fn push_vtec_row(target: &mut Vec<ProductVtecRow>, segment_index: Option<i32>, vtec: &VtecCode) {
    target.push(ProductVtecRow {
        segment_index,
        status: vtec.status.to_string(),
        action: vtec.action.clone(),
        office: vtec.office.clone(),
        phenomena: vtec.phenomena.clone(),
        significance: vtec.significance.to_string(),
        etn: i64::from(vtec.etn),
        begin_utc: vtec.begin,
        end_utc: vtec.end,
    });
}

fn push_hvtec_row(
    target: &mut Vec<ProductHvtecRow>,
    search_points: &mut Vec<ProductSearchPointRow>,
    segment_index: Option<i32>,
    hvtec_index: i32,
    hvtec: &HvtecCode,
) -> PersistResult<()> {
    let severity = serde_label(&hvtec.severity)?;
    let cause = serde_label(&hvtec.cause)?;
    let record = serde_label(&hvtec.record)?;
    let latitude = hvtec.location.as_ref().map(|location| location.latitude);
    let longitude = hvtec.location.as_ref().map(|location| location.longitude);

    target.push(ProductHvtecRow {
        segment_index,
        hvtec_index,
        nwslid: hvtec.nwslid.clone(),
        location_name: hvtec
            .location
            .as_ref()
            .map(|location| location.place_name.to_string()),
        severity: severity.clone(),
        cause: cause.clone(),
        record: record.clone(),
        begin_utc: hvtec.begin,
        crest_utc: hvtec.crest,
        end_utc: hvtec.end,
        latitude,
        longitude,
    });

    if let (Some(latitude), Some(longitude)) = (latitude, longitude) {
        search_points.push(ProductSearchPointRow {
            source_kind: "hvtec".to_string(),
            source_index: hvtec_index,
            latitude,
            longitude,
        });
    }

    Ok(())
}

fn push_time_mot_loc_row(
    target: &mut Vec<ProductTimeMotLocRow>,
    search_points: &mut Vec<ProductSearchPointRow>,
    segment_index: Option<i32>,
    entry_index: i32,
    entry: &TimeMotLocEntry,
) -> PersistResult<()> {
    target.push(ProductTimeMotLocRow {
        segment_index,
        entry_index,
        time_utc: entry.time_utc,
        direction_degrees: i32::from(entry.direction_degrees),
        speed_kt: i32::from(entry.speed_kt),
        path_wkt: entry.wkt.clone(),
    });

    for point in &entry.points {
        search_points.push(ProductSearchPointRow {
            source_kind: "time_mot_loc".to_string(),
            source_index: entry_index,
            latitude: point.0,
            longitude: point.1,
        });
    }

    Ok(())
}

fn flatten_header(header: Option<&ProductHeaderV2>) -> HeaderColumns {
    match header {
        Some(ProductHeaderV2::Afos {
            ttaaii,
            cccc,
            ddhhmm,
            bbb,
            afos,
        }) => HeaderColumns {
            header_kind: Some("afos".to_string()),
            ttaaii: Some(ttaaii.clone()),
            cccc: Some(cccc.clone()),
            ddhhmm: Some(ddhhmm.clone()),
            bbb: bbb.clone(),
            afos: Some(afos.clone()),
        },
        Some(ProductHeaderV2::Wmo {
            ttaaii,
            cccc,
            ddhhmm,
            bbb,
        }) => HeaderColumns {
            header_kind: Some("wmo".to_string()),
            ttaaii: Some(ttaaii.clone()),
            cccc: Some(cccc.clone()),
            ddhhmm: Some(ddhhmm.clone()),
            bbb: bbb.clone(),
            afos: None,
        },
        None => HeaderColumns {
            header_kind: None,
            ttaaii: None,
            cccc: None,
            ddhhmm: None,
            bbb: None,
            afos: None,
        },
    }
}

fn source_receiver(origin: &SourceKind) -> &'static str {
    match origin {
        SourceKind::Qbt => "qbt",
        SourceKind::WxWire { .. } => "wxwire",
        _ => "unknown",
    }
}

fn source_message_id(origin: &SourceKind) -> Option<String> {
    match origin {
        SourceKind::Qbt => None,
        SourceKind::WxWire { message_id, .. } => Some(message_id.clone()),
        _ => None,
    }
}

fn find_blob(blobs: &[StoredBlob], role: BlobRole) -> PersistResult<&StoredBlob> {
    blobs.iter().find(|blob| blob.role == role).ok_or_else(|| {
        PersistError::InvalidRequest(format!("missing required `{role:?}` blob reference"))
    })
}

fn find_blob_optional(blobs: &[StoredBlob], role: BlobRole) -> Option<&StoredBlob> {
    blobs.iter().find(|blob| blob.role == role)
}

fn usize_to_i32(value: usize, field: &str) -> PersistResult<i32> {
    i32::try_from(value).map_err(|_| {
        PersistError::InvalidRequest(format!("{field} value `{value}` exceeds i32 range"))
    })
}

fn serde_label<T: Serialize>(value: &T) -> PersistResult<String> {
    match serde_json::to_value(value)? {
        Value::String(value) => Ok(value),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        other => Err(PersistError::InvalidRequest(format!(
            "expected scalar label, found {other}"
        ))),
    }
}
