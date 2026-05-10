use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    process::Command,
};

use duckdb::Connection;
use rstest::fixture;

const EXPECTED_DATA_COLUMNS: &[&str] = &[
    "key_id",
    "sample_id",
    "band_id",
    "membership_id",
    "block_id",
    "value",
];
const EXPECTED_PROCESSED_CLASSES_COLUMNS: &[&str] = &["class_id", "class", "class_group"];
const EXPECTED_PROCESSED_MEMBERSHIPS_COLUMNS: &[&str] = &[
    "membership_id",
    "parent_id",
    "child_id",
    "collection",
    "parent_name",
    "parent_class",
    "parent_group",
    "parent_category",
    "child_name",
    "child_class",
    "child_group",
    "child_category",
    "kind",
];
const EXPECTED_PROCESSED_OBJECTS_COLUMNS: &[&str] =
    &["id", "name", "category", "class_group", "class"];
const EXPECTED_PROCESSED_PROPERTIES_COLUMNS: &[&str] = &[
    "property_id",
    "is_summary",
    "collection",
    "property",
    "unit",
];
const EXPECTED_TIMESTAMP_BLOCK_COLUMNS: &[&str] = &["block_id", "datetime", "interval_length"];
const EXPECTED_REPORT_PREFIX_COLUMNS: &[&str] = &[
    "band",
    "sample_name",
    "name",
    "category",
    "timestamp",
    "interval_length",
];
const EXPECTED_SCHEMAS: &[&str] = &["data", "main", "processed", "raw", "report"];
const EXPECTED_RAW_TABLES: &[(&str, &[&str])] = &[
    ("config", &["element", "value"]),
    (
        "memberships",
        &[
            "membership_id",
            "collection_id",
            "collection",
            "child_id",
            "child_name",
            "child_category",
            "child_category_class",
            "parent_id",
            "parent_name",
            "parent_category",
            "parent_category_class",
            "child_class_id",
            "child_class_name",
            "parent_class_id",
            "parent_class_name",
            "kind",
        ],
    ),
    (
        "collections",
        &[
            "collection_id",
            "parent_class_id",
            "child_class_id",
            "name",
            "complement_name",
        ],
    ),
    ("classes", &["class_id", "name", "class_group_id"]),
    ("class_groups", &["class_group_id", "name"]),
    ("categories", &["category_id", "class_id", "rank", "name"]),
    ("bands", &["band_id"]),
    ("models", &["model_id", "name"]),
    (
        "objects",
        &[
            "object_id",
            "class_id",
            "name",
            "category_id",
            "index",
            "is_show",
        ],
    ),
    (
        "keys",
        &[
            "key_id",
            "membership_id",
            "model_id",
            "phase_id",
            "property_id",
            "is_summary",
            "band_id",
            "sample_id",
            "timeslice_id",
        ],
    ),
    (
        "key_indexes",
        &[
            "key_id",
            "period_type_id",
            "position",
            "length",
            "period_offset",
        ],
    ),
    (
        "properties",
        &[
            "property_id",
            "name",
            "summary_name",
            "enum_id",
            "unit_id",
            "summary_unit_id",
            "is_multi_band",
            "is_period",
            "is_summary",
            "collection_id",
        ],
    ),
    ("timeslices", &["timeslice_id", "timeslice_name"]),
    (
        "samples",
        &[
            "sample_id",
            "sample_name",
            "sample_phase_id",
            "sample_weight",
        ],
    ),
    ("units", &["unit_id", "unit_name", "lang_id"]),
    ("memo_objects", &["value", "column_id", "object_id"]),
    (
        "custom_columns",
        &["column_id", "name", "position", "class_id"],
    ),
    ("attribute_data", &["object_id", "attribute_id", "value"]),
    (
        "attributes",
        &["attribute_id", "name", "lang_id", "class_id", "description"],
    ),
];

#[fixture]
fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/plexos-solution-files")
}

#[fixture]
fn temp_dir() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("create temp dir")
}

fn generated_output_path(temp_dir: &tempfile::TempDir, fixture_name: &str) -> PathBuf {
    temp_dir
        .path()
        .join(format!("{}.duckdb", fixture_name.replace(' ', "_")))
}

fn solution_zip_fixture_paths(fixture_dir: &Path) -> Vec<PathBuf> {
    let mut paths = std::fs::read_dir(fixture_dir)
        .expect("read fixture directory")
        .map(|entry| entry.expect("read fixture directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    paths
}

fn run_convert(source_path: &Path, output_path: &Path) {
    run_convert_with_args(source_path, output_path, &[]);
}

fn run_convert_with_args(source_path: &Path, output_path: &Path, extra_args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_plexos2duckdb"))
        .args([
            "convert",
            "--input",
            source_path.to_str().expect("fixture path utf8"),
            "--output",
            output_path.to_str().expect("output path utf8"),
            "--no-progress-bar",
        ])
        .args(extra_args)
        .output()
        .expect("run plexos2duckdb convert");

    assert!(
        output.status.success(),
        "conversion failed for {}\nstdout:\n{}\nstderr:\n{}",
        source_path.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn open_connection(db_path: &Path) -> Connection {
    Connection::open(db_path).expect("open generated duckdb file")
}

fn fetch_table_schemas(con: &Connection) -> BTreeSet<String> {
    con.prepare("SELECT DISTINCT table_schema FROM information_schema.tables ORDER BY table_schema")
        .expect("prepare schema query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("run schema query")
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .expect("collect schema rows")
}

fn fetch_metadata(con: &Connection) -> HashMap<String, String> {
    con.prepare("SELECT key, value FROM main.plexos2duckdb")
        .expect("prepare metadata query")
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })
        .expect("run metadata query")
        .collect::<std::result::Result<HashMap<_, _>, _>>()
        .expect("collect metadata rows")
}

fn fetch_table_names(con: &Connection, schema: &str, table_type: &str) -> Vec<String> {
    con.prepare(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = ? AND table_type = ? ORDER BY table_name",
    )
    .expect("prepare table listing query")
    .query_map([schema, table_type], |row| row.get::<_, String>(0))
    .expect("run table listing query")
    .collect::<std::result::Result<Vec<_>, _>>()
    .expect("collect table names")
}

fn count_parquet_files(path: &Path) -> usize {
    let mut count = 0usize;
    let mut pending = vec![path.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).expect("read parquet directory") {
            let entry = entry.expect("read parquet directory entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("parquet"))
            {
                count += 1;
            }
        }
    }
    count
}

fn fetch_column_names(con: &Connection, schema: &str, table: &str) -> Vec<String> {
    con.prepare(
        "SELECT column_name FROM information_schema.columns WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position",
    )
    .expect("prepare column listing query")
    .query_map([schema, table], |row| row.get::<_, String>(0))
    .expect("run column listing query")
    .collect::<std::result::Result<Vec<_>, _>>()
    .expect("collect column names")
}

fn assert_database_basics(db_path: &Path, source_path: &Path) {
    assert!(
        db_path.exists(),
        "expected output database at {}",
        db_path.display()
    );

    let con = open_connection(db_path);
    let schemas = fetch_table_schemas(&con);
    for schema in EXPECTED_SCHEMAS {
        assert!(
            schemas.contains(*schema),
            "missing schema {schema} in {} (actual: {:?})",
            db_path.display(),
            schemas
        );
    }

    let metadata = fetch_metadata(&con);
    assert_eq!(
        metadata.get("plexos_file").map(String::as_str),
        Some(source_path.to_string_lossy().as_ref()),
        "expected metadata source path to match fixture path"
    );
    assert!(
        metadata.contains_key("plexos2duckdb_version"),
        "missing converter version metadata"
    );
    assert!(
        metadata.contains_key("model_name"),
        "missing model name metadata"
    );

    let data_table_count: i64 = con
        .query_row(
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema='data' AND table_type='BASE TABLE'",
            [],
            |row| row.get(0),
        )
        .expect("count data tables");
    assert!(
        data_table_count > 0,
        "expected data tables in {}",
        db_path.display()
    );
}

fn assert_columns_exact(actual: &[String], expected: &[&str], context: &str) {
    let expected_vec = expected
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected_vec, "unexpected columns for {context}");
}

fn assert_report_view_shape(con: &Connection, view_name: &str, metric_name: &str) {
    let columns = fetch_column_names(con, "report", view_name);
    let mut expected = EXPECTED_REPORT_PREFIX_COLUMNS
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    expected.push(metric_name.to_string());
    expected.push("unit".to_string());
    assert_eq!(
        columns, expected,
        "unexpected columns for report.{view_name}"
    );
}

#[test]
fn all_solution_zip_fixtures_convert_end_to_end() {
    let fixture_dir = fixture_dir();
    let temp_dir = temp_dir();
    let fixture_paths = solution_zip_fixture_paths(&fixture_dir);

    assert!(
        !fixture_paths.is_empty(),
        "expected at least one ZIP fixture"
    );

    for source_path in fixture_paths {
        let fixture_name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture file name utf8");
        let output_path = generated_output_path(&temp_dir, fixture_name);

        run_convert(&source_path, &output_path);
        assert_database_basics(&output_path, &source_path);
    }
}

#[test]
fn day_ahead_database_matches_detailed_schema_expectations() {
    let fixture_name = "Model DAY_AHEAD Solution.zip";
    let fixture_dir = fixture_dir();
    let temp_dir = temp_dir();
    let source_path = fixture_dir.join(fixture_name);
    let output_path = generated_output_path(&temp_dir, fixture_name);

    run_convert(&source_path, &output_path);
    let con = open_connection(&output_path);

    let schemas = fetch_table_schemas(&con);
    for schema in EXPECTED_SCHEMAS {
        assert!(schemas.contains(*schema), "missing schema {schema}");
    }

    let metadata_table_exists: bool = con
        .query_row(
            "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_schema = 'main' AND table_name = 'plexos2duckdb'",
            [],
            |row| row.get(0),
        )
        .expect("check metadata table existence");
    assert!(metadata_table_exists, "missing main.plexos2duckdb");

    for (table, expected_columns) in EXPECTED_RAW_TABLES {
        let exists: bool = con
            .query_row(
                "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_schema = 'raw' AND table_name = ?",
                [*table],
                |row| row.get(0),
            )
            .expect("check raw table existence");
        assert!(exists, "missing raw.{table}");

        let actual_columns = fetch_column_names(&con, "raw", table);
        for column in *expected_columns {
            assert!(
                actual_columns.iter().any(|actual| actual == column),
                "raw.{table} missing column {column} (actual: {:?})",
                actual_columns
            );
        }
    }

    let processed_views = fetch_table_names(&con, "processed", "VIEW");
    assert!(processed_views.len() >= 3, "expected processed.* views");

    let report_views = fetch_table_names(&con, "report", "VIEW");
    assert!(!report_views.is_empty(), "expected report.* views");

    let data_tables = fetch_table_names(&con, "data", "BASE TABLE");
    assert!(!data_tables.is_empty(), "no data.* tables found");

    let mut non_empty = false;
    for table in data_tables.iter().take(5) {
        let count: i64 = con
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM \"data\".\"{}\"",
                    table.replace('"', "\"\"")
                ),
                [],
                |row| row.get(0),
            )
            .expect("count rows in data table");
        if count > 0 {
            non_empty = true;
            break;
        }
    }
    assert!(non_empty, "all sampled data tables were empty");

    let report_view = &report_views[0];
    let mut stmt = con
        .prepare(&format!(
            "SELECT * FROM \"report\".\"{}\" LIMIT 5",
            report_view.replace('"', "\"\"")
        ))
        .expect("prepare report view query");
    let mut rows = stmt.query([]).expect("query report view");
    assert!(
        rows.next().expect("read report row").is_some(),
        "expected report view to return at least one row"
    );
}

#[test]
fn convert_external_data_parquet_dir_creates_parquet_backed_data_views() {
    let fixture_name = "Model_Base_LT_Solution.zip";
    let fixture_dir = fixture_dir();
    let temp_dir = temp_dir();
    let source_path = fixture_dir.join(fixture_name);
    let output_path = generated_output_path(&temp_dir, fixture_name);
    let external_dir = temp_dir.path().join("external-data");
    let external_dir_arg = external_dir
        .to_str()
        .expect("external path utf8")
        .to_string();

    run_convert_with_args(
        &source_path,
        &output_path,
        &[
            "--table-name-pattern",
            "^LT__Interval__Batteries__(Generation|Load)$",
            "--external-data-parquet-dir",
            external_dir_arg.as_str(),
        ],
    );

    assert!(
        external_dir.join("data").is_dir(),
        "expected external data directory at {}",
        external_dir.join("data").display()
    );
    assert!(
        count_parquet_files(&external_dir) == 2,
        "expected external parquet files under {}",
        external_dir.display()
    );
    for table in [
        "LT__Interval__Batteries__Generation",
        "LT__Interval__Batteries__Load",
    ] {
        let expected_parquet_path = external_dir
            .join("data")
            .join(table)
            .join(format!("{table}.part-00001.parquet"));
        assert!(
            expected_parquet_path.exists(),
            "expected external parquet file at {}",
            expected_parquet_path.display()
        );
    }

    let con = open_connection(&output_path);
    let expected_tables = vec![
        "LT__Interval__Batteries__Generation".to_string(),
        "LT__Interval__Batteries__Load".to_string(),
    ];
    assert_eq!(
        fetch_table_names(&con, "data", "BASE TABLE"),
        Vec::<String>::new(),
        "external parquet mode should not materialize data base tables",
    );
    assert_eq!(
        fetch_table_names(&con, "data", "VIEW"),
        expected_tables,
        "unexpected external data view set",
    );
    assert_eq!(
        fetch_table_names(&con, "report", "VIEW"),
        expected_tables,
        "unexpected filtered report view set",
    );

    let root: String = con
        .query_row(
            "SELECT main.plexos2duckdb_external_data_root()",
            [],
            |row| row.get(0),
        )
        .expect("read external data root macro");
    assert_eq!(
        root,
        external_dir
            .canonicalize()
            .expect("canonical external path")
            .to_string_lossy()
            .replace('\\', "/"),
        "unexpected external data root macro value",
    );

    let count: i64 = con
        .query_row(
            "SELECT COUNT(*) FROM data.\"LT__Interval__Batteries__Generation\"",
            [],
            |row| row.get(0),
        )
        .expect("count external data view rows");
    assert_eq!(count, 144, "unexpected external data view row count");
}

#[test]
fn convert_filters_generated_data_tables_by_pattern() {
    let fixture_name = "Model_Base_LT_Solution.zip";
    let fixture_dir = fixture_dir();
    let temp_dir = temp_dir();
    let source_path = fixture_dir.join(fixture_name);
    let output_path = generated_output_path(&temp_dir, fixture_name);

    run_convert_with_args(
        &source_path,
        &output_path,
        &[
            "--table-name-pattern",
            "^LT__Interval__Batteries__Generation$",
        ],
    );
    let con = open_connection(&output_path);

    let expected_tables = vec!["LT__Interval__Batteries__Generation".to_string()];
    assert_eq!(
        fetch_table_names(&con, "data", "BASE TABLE"),
        expected_tables,
        "unexpected filtered data table set",
    );
    assert_eq!(
        fetch_table_names(&con, "report", "VIEW"),
        expected_tables,
        "unexpected filtered report view set",
    );

    let count: i64 = con
        .query_row(
            "SELECT COUNT(*) FROM data.\"LT__Interval__Batteries__Generation\"",
            [],
            |row| row.get(0),
        )
        .expect("count filtered data table rows");
    assert_eq!(count, 144, "unexpected filtered data row count");
}

#[test]
fn lt_solution_preserves_observed_table_shapes() {
    let fixture_name = "Model_Base_LT_Solution.zip";
    let fixture_dir = fixture_dir();
    let temp_dir = temp_dir();
    let source_path = fixture_dir.join(fixture_name);
    let output_path = generated_output_path(&temp_dir, fixture_name);

    run_convert(&source_path, &output_path);
    let con = open_connection(&output_path);

    let processed_views = fetch_table_names(&con, "processed", "VIEW");
    assert_eq!(
        processed_views,
        vec![
            "classes".to_string(),
            "memberships".to_string(),
            "objects".to_string(),
            "properties".to_string(),
            "timestamp_block_LT__Day".to_string(),
            "timestamp_block_LT__Interval".to_string(),
            "timestamp_block_LT__Year".to_string(),
        ],
        "unexpected processed views for LT solution",
    );

    assert_columns_exact(
        &fetch_column_names(&con, "processed", "classes"),
        EXPECTED_PROCESSED_CLASSES_COLUMNS,
        "processed.classes",
    );
    assert_columns_exact(
        &fetch_column_names(&con, "processed", "memberships"),
        EXPECTED_PROCESSED_MEMBERSHIPS_COLUMNS,
        "processed.memberships",
    );
    assert_columns_exact(
        &fetch_column_names(&con, "processed", "objects"),
        EXPECTED_PROCESSED_OBJECTS_COLUMNS,
        "processed.objects",
    );
    assert_columns_exact(
        &fetch_column_names(&con, "processed", "properties"),
        EXPECTED_PROCESSED_PROPERTIES_COLUMNS,
        "processed.properties",
    );
    assert_columns_exact(
        &fetch_column_names(&con, "processed", "timestamp_block_LT__Interval"),
        EXPECTED_TIMESTAMP_BLOCK_COLUMNS,
        "processed.timestamp_block_LT__Interval",
    );

    let data_tables = fetch_table_names(&con, "data", "BASE TABLE");
    assert_eq!(data_tables.len(), 29, "unexpected LT data table count");
    for table in [
        "LT__Interval__Batteries__Generation",
        "LT__Interval__Batteries__Load",
        "LT__Interval__Batteries__Net_Generation",
    ] {
        assert_columns_exact(
            &fetch_column_names(&con, "data", table),
            EXPECTED_DATA_COLUMNS,
            &format!("data.{table}"),
        );
        let count: i64 = con
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM \"data\".\"{}\"",
                    table.replace('"', "\"\"")
                ),
                [],
                |row| row.get(0),
            )
            .expect("count LT data table rows");
        assert_eq!(count, 144, "unexpected row count for data.{table}");
    }

    let report_views = fetch_table_names(&con, "report", "VIEW");
    assert_eq!(report_views.len(), 29, "unexpected LT report view count");
    assert_report_view_shape(&con, "LT__Interval__Batteries__Generation", "Generation");
    assert_report_view_shape(&con, "LT__Interval__Batteries__Load", "Load");
    assert_report_view_shape(
        &con,
        "LT__Interval__Batteries__Net_Generation",
        "Net_Generation",
    );
}

#[test]
fn st_solution_preserves_observed_table_shapes() {
    let fixture_name = "Model_Base_ST_Solution.zip";
    let fixture_dir = fixture_dir();
    let temp_dir = temp_dir();
    let source_path = fixture_dir.join(fixture_name);
    let output_path = generated_output_path(&temp_dir, fixture_name);

    run_convert(&source_path, &output_path);
    let con = open_connection(&output_path);

    let processed_views = fetch_table_names(&con, "processed", "VIEW");
    assert_eq!(
        processed_views,
        vec![
            "classes".to_string(),
            "memberships".to_string(),
            "objects".to_string(),
            "properties".to_string(),
            "timestamp_block_ST__Day".to_string(),
            "timestamp_block_ST__Interval".to_string(),
            "timestamp_block_ST__Year".to_string(),
        ],
        "unexpected processed views for ST solution",
    );

    assert_columns_exact(
        &fetch_column_names(&con, "processed", "timestamp_block_ST__Day"),
        EXPECTED_TIMESTAMP_BLOCK_COLUMNS,
        "processed.timestamp_block_ST__Day",
    );
    assert_columns_exact(
        &fetch_column_names(&con, "processed", "timestamp_block_ST__Interval"),
        EXPECTED_TIMESTAMP_BLOCK_COLUMNS,
        "processed.timestamp_block_ST__Interval",
    );

    let data_tables = fetch_table_names(&con, "data", "BASE TABLE");
    assert_eq!(data_tables.len(), 53, "unexpected ST data table count");
    for table in [
        "ST__Interval__Batteries__Generation",
        "ST__Interval__Batteries__Load",
        "ST__Interval__Batteries__Net_Generation",
    ] {
        assert_columns_exact(
            &fetch_column_names(&con, "data", table),
            EXPECTED_DATA_COLUMNS,
            &format!("data.{table}"),
        );
        let count: i64 = con
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM \"data\".\"{}\"",
                    table.replace('"', "\"\"")
                ),
                [],
                |row| row.get(0),
            )
            .expect("count ST data table rows");
        assert_eq!(count, 24, "unexpected row count for data.{table}");
    }

    let report_views = fetch_table_names(&con, "report", "VIEW");
    assert_eq!(report_views.len(), 53, "unexpected ST report view count");
    assert_report_view_shape(&con, "ST__Interval__Batteries__Generation", "Generation");
    assert_report_view_shape(&con, "ST__Interval__Batteries__Load", "Load");
    assert_report_view_shape(
        &con,
        "ST__Interval__Batteries__Net_Generation",
        "Net_Generation",
    );
}

#[test]
fn base_solution_preserves_observed_table_shapes() {
    let fixture_name = "Model_Base_Solution.zip";
    let fixture_dir = fixture_dir();
    let temp_dir = temp_dir();
    let source_path = fixture_dir.join(fixture_name);
    let output_path = generated_output_path(&temp_dir, fixture_name);

    run_convert(&source_path, &output_path);
    let con = open_connection(&output_path);

    let processed_views = fetch_table_names(&con, "processed", "VIEW");
    assert_eq!(
        processed_views,
        vec![
            "classes".to_string(),
            "memberships".to_string(),
            "objects".to_string(),
            "properties".to_string(),
            "timestamp_block_ST__Day".to_string(),
            "timestamp_block_ST__Interval".to_string(),
        ],
        "unexpected processed views for base solution",
    );

    assert_columns_exact(
        &fetch_column_names(&con, "processed", "classes"),
        EXPECTED_PROCESSED_CLASSES_COLUMNS,
        "processed.classes",
    );
    assert_columns_exact(
        &fetch_column_names(&con, "processed", "timestamp_block_ST__Interval"),
        EXPECTED_TIMESTAMP_BLOCK_COLUMNS,
        "processed.timestamp_block_ST__Interval",
    );

    let data_tables = fetch_table_names(&con, "data", "BASE TABLE");
    assert_eq!(data_tables.len(), 43, "unexpected base data table count");
    for table in [
        "ST__Day__Batteries__Generation",
        "ST__Day__Batteries__Load",
        "ST__Day__Batteries__Net_Generation",
    ] {
        assert_columns_exact(
            &fetch_column_names(&con, "data", table),
            EXPECTED_DATA_COLUMNS,
            &format!("data.{table}"),
        );
        let count: i64 = con
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM \"data\".\"{}\"",
                    table.replace('"', "\"\"")
                ),
                [],
                |row| row.get(0),
            )
            .expect("count base data table rows");
        assert_eq!(count, 1, "unexpected row count for data.{table}");
    }

    let report_views = fetch_table_names(&con, "report", "VIEW");
    assert_eq!(report_views.len(), 43, "unexpected base report view count");
    assert_report_view_shape(&con, "ST__Day__Batteries__Generation", "Generation");
    assert_report_view_shape(&con, "ST__Day__Batteries__Load", "Load");
    assert_report_view_shape(
        &con,
        "ST__Interval__Regions__Unserved_Energy",
        "Unserved_Energy",
    );
}
