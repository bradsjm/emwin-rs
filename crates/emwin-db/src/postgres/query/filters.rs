//! Non-spatial SQL predicate construction for archive queries.

use super::super::PersistResult;
use super::spatial::append_spatial_filter;
use super::{
    FacetDimension, ProductListQuery, normalize_lower, normalize_upper, split_csv_i64,
    split_csv_values,
};
use sqlx::{Postgres, QueryBuilder};

pub(super) fn append_product_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    query: &ProductListQuery,
) -> PersistResult<()> {
    append_like_filter(builder, "products.filename", query.filename.as_deref());
    append_text_set_filter(
        builder,
        "products.source_receiver",
        query.source_receiver.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.source",
        query.source.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.pil",
        query.pil.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.family",
        query.family.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.artifact_kind",
        query.artifact_kind.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.container",
        query.container.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.wmo_prefix",
        query.wmo_prefix.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.office_code",
        query.office.as_deref(),
        normalize_upper,
    );
    append_case_insensitive_text_set_filter(
        builder,
        "products.office_city",
        query.office_city.as_deref(),
    );
    append_text_set_filter(
        builder,
        "products.office_state",
        query.office_state.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.bbb_kind",
        query.bbb_kind.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.cccc",
        query.cccc.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.ttaaii",
        query.ttaaii.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.afos",
        query.afos.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.bbb",
        query.bbb.as_deref(),
        normalize_upper,
    );
    append_bool_filter(builder, "products.has_issues", query.has_issues);
    append_bool_filter(builder, "products.has_vtec", query.has_vtec);
    append_bool_filter(builder, "products.has_ugc", query.has_ugc);
    append_bool_filter(builder, "products.has_hvtec", query.has_hvtec);
    append_bool_filter(builder, "products.has_latlon", query.has_latlon);
    append_bool_filter(builder, "products.has_time_mot_loc", query.has_time_mot_loc);
    append_bool_filter(builder, "products.has_wind_hail", query.has_wind_hail);

    if let Some(min_size) = query.min_size {
        builder
            .push(" AND products.size_bytes >= ")
            .push_bind(i64::try_from(min_size).expect("size should fit in i64"));
    }
    if let Some(max_size) = query.max_size {
        builder
            .push(" AND products.size_bytes <= ")
            .push_bind(i64::try_from(max_size).expect("size should fit in i64"));
    }
    if let Some(after) = query.source_timestamp_after {
        builder
            .push(" AND products.source_timestamp_utc >= ")
            .push_bind(after);
    }
    if let Some(before) = query.source_timestamp_before {
        builder
            .push(" AND products.source_timestamp_utc <= ")
            .push_bind(before);
    }
    if let Some(after) = query.ingested_after {
        builder
            .push(" AND products.ingested_at >= ")
            .push_bind(after);
    }
    if let Some(before) = query.ingested_before {
        builder
            .push(" AND products.ingested_at <= ")
            .push_bind(before);
    }

    if let Some(states) = split_csv_values(query.state.as_deref(), normalize_upper) {
        builder.push(" AND EXISTS (SELECT 1 FROM product_ugc_areas WHERE product_ugc_areas.product_id = products.id AND ");
        append_in_clause(builder, "product_ugc_areas.state", states);
        builder.push(")");
    }

    append_ugc_exists(builder, query.county.as_deref(), "county");
    append_ugc_exists(builder, query.zone.as_deref(), "zone");
    append_ugc_exists(builder, query.fire_zone.as_deref(), "fire_zone");
    append_ugc_exists(builder, query.marine_zone.as_deref(), "marine_zone");

    if query.issue_kind.is_some() || query.issue_code.is_some() {
        builder.push(
            " AND EXISTS (SELECT 1 FROM product_issues WHERE product_issues.product_id = products.id",
        );
        append_issue_alias_filters(builder, "product_issues", query);
        builder.push(")");
    }

    if query.vtec_phenomena.is_some()
        || query.vtec_significance.is_some()
        || query.vtec_action.is_some()
        || query.vtec_office.is_some()
        || query.etn.is_some()
    {
        builder.push(
            " AND EXISTS (SELECT 1 FROM product_vtec WHERE product_vtec.product_id = products.id",
        );
        append_vtec_alias_filters(builder, "product_vtec", query)?;
        builder.push(")");
    }

    if query.hvtec_nwslid.is_some()
        || query.hvtec_severity.is_some()
        || query.hvtec_cause.is_some()
        || query.hvtec_record.is_some()
    {
        builder.push(
            " AND EXISTS (SELECT 1 FROM product_hvtec WHERE product_hvtec.product_id = products.id",
        );
        if let Some(values) = split_csv_values(query.hvtec_nwslid.as_deref(), normalize_upper) {
            builder.push(" AND ");
            append_in_clause(builder, "product_hvtec.nwslid", values);
        }
        if let Some(values) = split_csv_values(query.hvtec_severity.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_hvtec.severity", values);
        }
        if let Some(values) = split_csv_values(query.hvtec_cause.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_hvtec.cause", values);
        }
        if let Some(values) = split_csv_values(query.hvtec_record.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_hvtec.record", values);
        }
        builder.push(")");
    }

    if query.wind_hail_kind.is_some()
        || query.min_wind_mph.is_some()
        || query.min_hail_inches.is_some()
    {
        builder.push(
            " AND EXISTS (SELECT 1 FROM product_wind_hail WHERE product_wind_hail.product_id = products.id",
        );
        if let Some(values) = split_csv_values(query.wind_hail_kind.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_wind_hail.kind", values);
        }
        if let Some(min_wind_mph) = query.min_wind_mph {
            builder.push(
                " AND product_wind_hail.kind IN ('legacy_wind', 'max_wind_gust') AND CASE WHEN UPPER(COALESCE(product_wind_hail.units, '')) IN ('KTS', 'KT') THEN product_wind_hail.numeric_value * 1.15078 ELSE product_wind_hail.numeric_value END >= ",
            )
            .push_bind(min_wind_mph);
        }
        if let Some(min_hail_inches) = query.min_hail_inches {
            builder.push(
                " AND product_wind_hail.kind IN ('legacy_hail', 'max_hail_size') AND product_wind_hail.numeric_value >= ",
            )
            .push_bind(min_hail_inches);
        }
        builder.push(")");
    }

    append_spatial_filter(builder, query)?;
    Ok(())
}

pub(super) fn append_issue_alias_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    alias: &str,
    query: &ProductListQuery,
) {
    if let Some(kinds) = split_csv_values(query.issue_kind.as_deref(), normalize_lower) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.kind"), kinds);
    }
    if let Some(codes) = split_csv_values(query.issue_code.as_deref(), normalize_lower) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.code"), codes);
    }
}

pub(super) fn append_vtec_alias_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    alias: &str,
    query: &ProductListQuery,
) -> PersistResult<()> {
    if let Some(values) = split_csv_values(query.vtec_phenomena.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.phenomena"), values);
    }
    if let Some(values) = split_csv_values(query.vtec_significance.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.significance"), values);
    }
    if let Some(values) = split_csv_values(query.vtec_action.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.action"), values);
    }
    if let Some(values) = split_csv_values(query.vtec_office.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.office"), values);
    }
    if let Some(values) = split_csv_i64(query.etn.as_deref())? {
        builder.push(" AND ");
        append_in_clause_i64(builder, &format!("{alias}.etn"), values);
    }
    Ok(())
}

pub(super) fn append_facet_non_null_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    dimension: FacetDimension,
) {
    match dimension {
        FacetDimension::Office => builder.push(" AND products.office_code IS NOT NULL"),
        FacetDimension::Family => builder.push(" AND products.family IS NOT NULL"),
        FacetDimension::ArtifactKind => builder.push(" AND products.artifact_kind IS NOT NULL"),
        FacetDimension::Phenomena => builder.push(" AND facet.phenomena IS NOT NULL"),
        FacetDimension::Significance => builder.push(" AND facet.significance IS NOT NULL"),
        FacetDimension::Status => builder.push(" AND facet.current_status IS NOT NULL"),
        FacetDimension::IssueKind => builder.push(" AND facet.kind IS NOT NULL"),
        FacetDimension::IssueCode => builder.push(" AND facet.code IS NOT NULL"),
    };
}

fn append_ugc_exists(
    builder: &mut QueryBuilder<'_, Postgres>,
    raw_values: Option<&str>,
    normalized_kind: &'static str,
) {
    let Some(values) = split_csv_values(raw_values, normalize_upper) else {
        return;
    };
    builder.push(
        " AND EXISTS (SELECT 1 FROM product_ugc_areas WHERE product_ugc_areas.product_id = products.id AND product_ugc_areas.area_kind = ",
    );
    builder.push_bind(normalized_kind);
    builder.push(" AND ");
    append_in_clause(builder, "product_ugc_areas.ugc_code", values);
    builder.push(")");
}

fn append_like_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    column: &str,
    raw_value: Option<&str>,
) {
    let Some(raw_value) = raw_value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let pattern = raw_value.replace('*', "%");
    builder
        .push(" AND ")
        .push(column)
        .push(" ILIKE ")
        .push_bind(pattern);
}

fn append_bool_filter(builder: &mut QueryBuilder<'_, Postgres>, column: &str, value: Option<bool>) {
    if let Some(value) = value {
        builder
            .push(" AND ")
            .push(column)
            .push(" = ")
            .push_bind(value);
    }
}

fn append_text_set_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    column: &str,
    raw_values: Option<&str>,
    normalize: fn(&str) -> String,
) {
    let Some(values) = split_csv_values(raw_values, normalize) else {
        return;
    };
    builder.push(" AND ");
    append_in_clause(builder, column, values);
}

fn append_case_insensitive_text_set_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    column: &str,
    raw_values: Option<&str>,
) {
    let Some(values) = split_csv_values(raw_values, normalize_lower) else {
        return;
    };
    builder.push(" AND LOWER(").push(column).push(") IN (");
    let mut separated = builder.separated(", ");
    for value in values {
        separated.push_bind(value);
    }
    separated.push_unseparated(")");
}

fn append_in_clause(builder: &mut QueryBuilder<'_, Postgres>, column: &str, values: Vec<String>) {
    builder.push(column).push(" IN (");
    let mut separated = builder.separated(", ");
    for value in values {
        separated.push_bind(value);
    }
    separated.push_unseparated(")");
}

fn append_in_clause_i64(builder: &mut QueryBuilder<'_, Postgres>, column: &str, values: Vec<i64>) {
    builder.push(column).push(" IN (");
    let mut separated = builder.separated(", ");
    for value in values {
        separated.push_bind(value);
    }
    separated.push_unseparated(")");
}
