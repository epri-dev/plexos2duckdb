#![allow(dead_code)]
#![doc = include_str!("../README.md")]

use std::io::Read as _;
use std::sync::{Arc, LazyLock};

use color_eyre::{Result, eyre::eyre};
use duckdb::arrow::{
    array::{ArrayRef, Float64Array, Int64Array},
    datatypes::{DataType, Field, Schema, SchemaRef},
    record_batch::RecordBatch,
};
use parquet::{arrow::ArrowWriter, basic::Compression, file::properties::WriterProperties};
use roxmltree::{Document, Node};

mod deflate_index;
pub mod utils;

pub const DEFAULT_DEFLATE_INDEX_INTERVAL_MIB: u64 = 64;

const DATA_APPEND_BATCH_VALUES: u64 = 262_144;
const DATA_RANGE_TASK_VALUES: u64 = 8_388_608;
const MIB_BYTES: u64 = 1024 * 1024;
const MIN_DEFLATE_INDEX_INTERVAL_BYTES: u64 = MIB_BYTES;

static DATA_RECORD_BATCH_SCHEMA: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("key_id", DataType::Int64, false),
        Field::new("sample_id", DataType::Int64, false),
        Field::new("band_id", DataType::Int64, false),
        Field::new("membership_id", DataType::Int64, false),
        Field::new("block_id", DataType::Int64, false),
        Field::new("value", DataType::Float64, false),
    ]))
});

#[derive(Debug, Default, Clone)]
struct Unit {
    id: i64,
    value: String,
    lang_id: i64,
}

type Band = i64;

#[derive(Debug, Default, Clone)]
struct Category {
    category_id: i64,
    name: String,
    rank: i64,
    class_id: i64,
}

#[derive(Debug, Default, Clone)]
struct Membership {
    membership_id: i64,
    parent_class_id: i64,
    child_class_id: i64,
    collection_id: i64,
    parent_object_id: i64,
    child_object_id: i64,
    // additional metadata
    collection_idx: usize,
}

#[derive(Debug, Default, Clone)]
struct Model {
    model_id: i64,
    name: String,
}

#[derive(Debug, Default, Clone)]
struct Object {
    object_id: i64,
    name: String,
    index: i64,
    show: bool,
    class_id: i64,
    category_id: i64,
    guid: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct Class {
    class_id: i64,
    name: String,
    state: Option<i64>,
    lang_id: i64,
    class_group_id: i64,
}

#[derive(Debug, Default, Clone)]
struct ClassGroup {
    class_group_id: i64,
    name: String,
    lang_id: i64,
    state: Option<i64>,
}

#[derive(Debug, Default, Clone, Hash, PartialEq, Eq)]
struct Collection {
    collection_id: i64,
    name: String,
    lang_id: i64,
    complement_name: Option<String>,
    parent_class_id: i64,
    child_class_id: i64,
    // additional metadata
    n_members: usize,
}

#[derive(Debug, Clone)]
struct Period0 {
    interval_id: i64,
    period_of_day: i64,
    hour_id: i64,
    day_id: i64,
    week_id: i64,
    month_id: i64,
    quarter_id: Option<i64>,
    fiscal_year_id: i64,
    datetime: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
struct Period1 {
    day_id: i64,
    week_id: i64,
    month_id: i64,
    fiscal_year_id: i64,
    quarter_id: Option<i64>,
    date: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
struct Period2 {
    week_id: i64,
    week_ending: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
struct Period3 {
    month_id: i64,
    month_beginning: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
struct Period4 {
    fiscal_year_id: i64,
    year_ending: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
struct Period6 {
    hour_id: i64,
    datetime: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
struct Period7 {
    quarter_id: i64,
    quarter_beginning: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
enum PeriodType {
    Interval(Period0),
    Day(Period1),
    Week(Period2),
    Month(Period3),
    Year(Period4),
    Hour(Period6),
    Quarter(Period7),
}

impl PeriodType {
    fn name(&self) -> String {
        match self {
            PeriodType::Interval(_) => "Interval".to_string(),
            PeriodType::Day(_) => "Day".to_string(),
            PeriodType::Week(_) => "Week".to_string(),
            PeriodType::Month(_) => "Month".to_string(),
            PeriodType::Year(_) => "Year".to_string(),
            PeriodType::Hour(_) => "Hour".to_string(),
            PeriodType::Quarter(_) => "Quarter".to_string(),
        }
    }

    fn datetime(&self) -> chrono::DateTime<chrono::Utc> {
        match self {
            PeriodType::Interval(p) => p.datetime,
            PeriodType::Day(p) => p.date,
            PeriodType::Week(p) => p.week_ending,
            PeriodType::Month(p) => p.month_beginning,
            PeriodType::Year(p) => p.year_ending,
            PeriodType::Hour(p) => p.datetime,
            PeriodType::Quarter(p) => p.quarter_beginning,
        }
    }
}

#[derive(Debug, Default, Clone)]
struct Phase {
    interval_id: i64,
    period_id: i64,
}
type Phase1 = Phase; // LT period
type Phase2 = Phase; // PASA period
type Phase3 = Phase; // MT period
type Phase4 = Phase; // ST period

#[derive(Debug, Clone)]
enum PhaseType {
    LT(Phase),
    PASA(Phase),
    MT(Phase),
    ST(Phase),
}

impl PhaseType {
    fn name(&self) -> String {
        match self {
            PhaseType::LT(_) => "LT".to_string(),
            PhaseType::PASA(_) => "PASA".to_string(),
            PhaseType::MT(_) => "MT".to_string(),
            PhaseType::ST(_) => "ST".to_string(),
        }
    }

    fn interval_id(&self) -> i64 {
        match self {
            PhaseType::LT(p) => p.interval_id,
            PhaseType::PASA(p) => p.interval_id,
            PhaseType::MT(p) => p.interval_id,
            PhaseType::ST(p) => p.interval_id,
        }
    }

    fn period_id(&self) -> i64 {
        match self {
            PhaseType::LT(p) => p.period_id,
            PhaseType::PASA(p) => p.period_id,
            PhaseType::MT(p) => p.period_id,
            PhaseType::ST(p) => p.period_id,
        }
    }
}

#[derive(Debug, Default, Clone)]
struct Sample {
    sample_id: i64,
    name: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct SampleWeight {
    sample_id: i64,
    phase_id: i64,
    weight: f64,
}

#[derive(Debug, Default, Clone)]
struct Timeslice {
    timeslice_id: i64,
    name: String,
}

#[derive(Debug, Default, Clone)]
struct Key {
    key_id: i64,
    phase_id: i64,
    // period_type_id is 1 for summary keys and 0 for non-summary keys
    // renamed to is_summary for clarity
    is_summary: bool,
    band_id: i64,
    membership_id: i64,
    model_id: i64,
    property_id: i64,
    sample_id: i64,
    timeslice_id: i64,
}

type PeriodTypeId = i64;
type KeyId = i64;

#[derive(Debug, Default, Clone)]
struct KeyIndex {
    key_id: KeyId,                // key_id
    period_type_id: PeriodTypeId, // period_type_id
    length: u64,                  // in 8-byte (64-bit float) increments
    position: u64,                // bytes from binary file start
    period_offset: i64,           // temporal data offset (if any) in stored times
}

#[derive(Debug, Default, Clone)]
struct AttributeData {
    object_id: Option<i64>,
    attribute_id: i64,
    value: f64,
}

#[derive(Debug, Default, Clone)]
struct Attribute {
    attribute_id: i64,
    name: String,
    description: String,
    enum_id: i64,
    lang_id: i64,
    class_id: i64,
    input_mask: Option<String>,
}

#[derive(Debug, Default, Clone, Hash, PartialEq, Eq)]
struct Property {
    property_id: i64,
    name: String,
    summary_name: String,
    lang_id: i64,
    enum_id: i64,
    is_multi_band: bool,
    is_period: bool,
    is_summary: bool,
    unit_id: i64,
    summary_unit_id: i64,
    collection_id: i64,
    // additional metadata
    band_id: i64,
}

impl Property {
    fn property_name(&self) -> String {
        self.name.clone()
    }

    fn summary_name(&self) -> String {
        if self.is_summary {
            self.summary_name.clone()
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Default, Clone)]
struct CustomColumn {
    column_id: i64,
    name: String,
    position: i64,
    class_id: i64,
}

#[derive(Debug, Default, Clone)]
struct MemoObject {
    value: String,
    column_id: i64,
    object_id: i64,
}

#[derive(Debug)]
struct ZipPeriodData {
    archive_file: Arc<std::fs::File>,
    data_start: u64,
    data_len: u64,
}

impl ZipPeriodData {
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        let read_len = u64::try_from(buf.len()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "buffer length does not fit in u64",
            )
        })?;
        let end_offset = offset.checked_add(read_len).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "period data read offset overflow",
            )
        })?;

        if end_offset > self.data_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "period data read exceeds ZIP entry bounds",
            ));
        }

        let archive_offset = self.data_start.checked_add(offset).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "archive read offset overflow",
            )
        })?;

        SolutionDataset::read_exact_at(&self.archive_file, archive_offset, buf)
    }
}

#[derive(Debug, Clone)]
struct ZipMemberReader {
    archive_file: Arc<std::fs::File>,
    data_start: u64,
    compressed_len: u64,
    position: u64,
}

impl ZipMemberReader {
    fn new(archive_file: Arc<std::fs::File>, data_start: u64, compressed_len: u64) -> Self {
        Self {
            archive_file,
            data_start,
            compressed_len,
            position: 0,
        }
    }
}

impl std::io::Read for ZipMemberReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() || self.position >= self.compressed_len {
            return Ok(0);
        }

        let remaining = self.compressed_len - self.position;
        let read_len = remaining.min(buf.len() as u64) as usize;
        let archive_offset = self.data_start.checked_add(self.position).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "archive read offset overflow",
            )
        })?;
        SolutionDataset::read_exact_at(&self.archive_file, archive_offset, &mut buf[..read_len])?;
        self.position = self.position.checked_add(read_len as u64).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ZIP member position overflow",
            )
        })?;
        Ok(read_len)
    }
}

impl std::io::Seek for ZipMemberReader {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let next = match pos {
            std::io::SeekFrom::Start(offset) => i128::from(offset),
            std::io::SeekFrom::End(offset) => i128::from(self.compressed_len) + i128::from(offset),
            std::io::SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
        };

        if next < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot seek before ZIP member start",
            ));
        }

        self.position = u64::try_from(next).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ZIP member seek offset exceeds u64",
            )
        })?;
        Ok(self.position)
    }
}

#[derive(Debug)]
struct CompressedZipPeriodData {
    archive_file: Arc<std::fs::File>,
    data_start: u64,
    compressed_len: u64,
    data_len: u64,
    name: String,
}

impl CompressedZipPeriodData {
    fn compressed_reader(&self) -> ZipMemberReader {
        ZipMemberReader::new(
            self.archive_file.clone(),
            self.data_start,
            self.compressed_len,
        )
    }

    fn build_deflate_index(&self, interval_bytes: u64) -> Result<CompressedDeflateIndex> {
        let interval_bytes = interval_bytes.max(MIN_DEFLATE_INDEX_INTERVAL_BYTES);
        let index = deflate_index::DeflateIndex::build(
            self.compressed_reader(),
            self.data_len,
            interval_bytes,
        )?;
        Ok(CompressedDeflateIndex { index })
    }

    fn read_exact_at_with_index(
        &self,
        index: &CompressedDeflateIndex,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        index
            .index
            .read_exact_at(self.compressed_reader(), offset, buf)?;
        Ok(())
    }

    fn indexed_reader<'a>(
        &self,
        index: &'a CompressedDeflateIndex,
        offset: u64,
    ) -> std::io::Result<deflate_index::DeflateIndexedReader<'a, ZipMemberReader>> {
        index.index.indexed_reader(self.compressed_reader(), offset)
    }

    fn read_exact_at(&self, _offset: u64, _buf: &mut [u8]) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "compressed ZIP period data requires a deflate seek index",
        ))
    }
}

#[derive(Debug, Clone)]
struct CompressedDeflateIndex {
    index: deflate_index::DeflateIndex,
}

#[derive(Debug)]
struct CompressedDeflateIndexes {
    by_period_type_id: DeflateIndexMap,
}

impl CompressedDeflateIndexes {
    fn get(&self, period_type_id: i64) -> Option<&CompressedDeflateIndex> {
        self.by_period_type_id
            .get(&period_type_id)
            .map(std::convert::AsRef::as_ref)
    }
}

#[derive(Debug, Clone)]
struct DataRangeWriteTask {
    table_name: String,
    key_id: i64,
    sample_id: i64,
    band_id: i64,
    membership_id: i64,
    period_type_id: i64,
    position: u64,
    start_value_index: u64,
    value_count: u64,
    period_offset: i64,
}

#[derive(Debug)]
struct StagedDataFiles {
    table_files: std::collections::BTreeMap<String, Vec<std::path::PathBuf>>,
}

#[derive(Debug)]
enum DataRangeWriteWorkerEvent {
    RangeStarted {
        worker_id: usize,
        index: usize,
        total: usize,
        table_name: String,
        key_id: i64,
        start_value: u64,
        values: u64,
    },
    RangeCompleted {
        worker_id: usize,
        index: usize,
        total: usize,
    },
}

#[derive(Debug, Clone, Copy)]
struct DataValueMetadata {
    key_id: i64,
    sample_id: i64,
    band_id: i64,
    membership_id: i64,
    period_offset: i64,
    start_value_index: u64,
}

impl DataValueMetadata {
    fn block_id(&self, chunk_index: usize) -> Result<i64> {
        let chunk_index = u64::try_from(chunk_index).map_err(|_| {
            eyre!(
                "block_id chunk index exceeds u64 for key_id {}",
                self.key_id
            )
        })?;
        let block_idx = self
            .start_value_index
            .checked_add(chunk_index)
            .ok_or_else(|| eyre!("block_id overflow for key_id {}", self.key_id))?;
        i64::try_from(block_idx)
            .map_err(|_| eyre!("block_id exceeds i64 for key_id {}", self.key_id))?
            .checked_add(self.period_offset)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| eyre!("block_id overflow for key_id {}", self.key_id))
    }
}

impl PeriodData {
    fn is_compressed_zip_entry(&self) -> bool {
        matches!(self, Self::CompressedZipEntry(_))
    }

    fn compressed_zip_entry(&self) -> Option<&CompressedZipPeriodData> {
        match self {
            Self::CompressedZipEntry(entry) => Some(entry),
            _ => None,
        }
    }
}

type DeflateIndexMap = std::collections::HashMap<i64, Arc<CompressedDeflateIndex>>;

impl DataRangeWriteTask {
    fn estimated_values(&self) -> u128 {
        u128::from(self.value_count)
    }
}

impl StagedDataFiles {
    fn new(table_files: std::collections::BTreeMap<String, Vec<std::path::PathBuf>>) -> Self {
        Self { table_files }
    }
}

impl DataRangeWriteWorkerEvent {
    fn completed(worker_id: usize, index: usize, total: usize) -> Self {
        Self::RangeCompleted {
            worker_id,
            index,
            total,
        }
    }
}

impl DataValueMetadata {
    fn record_batch(&self, bytes: &[u8]) -> Result<RecordBatch> {
        if bytes.len() % 8 != 0 {
            return Err(eyre!(
                "Data chunk byte length is not a multiple of f64 width for key_id {}",
                self.key_id
            ));
        }

        let value_count = bytes.len() / 8;
        let mut values = Vec::with_capacity(value_count);
        for chunk in bytes.chunks_exact(8) {
            values.push(f64::from_le_bytes(
                chunk.try_into().expect("chunks_exact gives eight bytes"),
            ));
        }

        let mut block_ids = Vec::with_capacity(value_count);
        for idx in 0..value_count {
            block_ids.push(self.block_id(idx)?);
        }

        let columns: Vec<ArrayRef> = vec![
            Arc::new(Int64Array::from(vec![self.key_id; value_count])),
            Arc::new(Int64Array::from(vec![self.sample_id; value_count])),
            Arc::new(Int64Array::from(vec![self.band_id; value_count])),
            Arc::new(Int64Array::from(vec![self.membership_id; value_count])),
            Arc::new(Int64Array::from(block_ids)),
            Arc::new(Float64Array::from(values)),
        ];
        Ok(RecordBatch::try_new(
            DATA_RECORD_BATCH_SCHEMA.clone(),
            columns,
        )?)
    }
}

#[derive(Debug)]
enum PeriodData {
    File(std::fs::File),
    ZipEntry(ZipPeriodData),
    CompressedZipEntry(CompressedZipPeriodData),
}

impl PeriodData {
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        match self {
            Self::File(file) => SolutionDataset::read_exact_at(file, offset, buf),
            Self::ZipEntry(entry) => entry.read_exact_at(offset, buf),
            Self::CompressedZipEntry(entry) => entry.read_exact_at(offset, buf),
        }
    }
}

/// Container for all PLEXOS solution data
#[derive(Debug, Default)]
pub struct SolutionDataset {
    file: std::path::PathBuf,
    model_name: String,
    attribute_data: indexmap::IndexMap<i64, AttributeData>,
    attribute: indexmap::IndexMap<i64, Attribute>,
    band: indexmap::IndexMap<i64, Band>,
    category: indexmap::IndexMap<i64, Category>,
    class_group: indexmap::IndexMap<i64, ClassGroup>,
    class: indexmap::IndexMap<i64, Class>,
    collection: indexmap::IndexMap<i64, Collection>,
    membership: indexmap::IndexMap<i64, Membership>,
    config: indexmap::IndexMap<String, Option<String>>,
    key_index: indexmap::IndexMap<i64, KeyIndex>,
    key: indexmap::IndexMap<i64, Key>,
    model: indexmap::IndexMap<i64, Model>,
    object: indexmap::IndexMap<i64, Object>,
    period: std::collections::HashMap<String, indexmap::IndexMap<i64, PeriodType>>,
    property: indexmap::IndexMap<i64, Property>,
    phase: std::collections::HashMap<String, indexmap::IndexMap<i64, PhaseType>>,
    sample: indexmap::IndexMap<i64, Sample>,
    sample_weight: indexmap::IndexMap<i64, SampleWeight>,
    timeslice: indexmap::IndexMap<i64, Timeslice>,
    unit: indexmap::IndexMap<i64, Unit>,
    memo_object: Vec<MemoObject>,
    custom_column: indexmap::IndexMap<i64, CustomColumn>,
    period_data: indexmap::IndexMap<i64, PeriodData>,
    simulation_log: Option<String>,
    run_stats: Option<String>,
    // calculated fields
    timestamp_block: std::collections::HashMap<String, Vec<(chrono::DateTime<chrono::Utc>, i64)>>,
    table_key_index_mapping: std::collections::HashMap<String, Vec<i64>>,
    table_units_mapping: std::collections::HashMap<String, (String, i64)>,
}

#[derive(Debug, Clone)]
pub enum ProgressEvent {
    DataTableStart {
        index: usize,
        total: usize,
        table_name: String,
        keys: usize,
    },
    DataTableEnd,
    DataWorkerTableStart {
        worker_id: usize,
        index: usize,
        total: usize,
        table_name: String,
        keys: usize,
    },
    DataWorkerTableEnd {
        worker_id: usize,
        index: usize,
        total: usize,
    },
    DataWorkerRangeStart {
        worker_id: usize,
        index: usize,
        total: usize,
        table_name: String,
        key_id: i64,
        start_value: u64,
        values: u64,
    },
    DataWorkerRangeEnd {
        worker_id: usize,
        index: usize,
        total: usize,
    },
    DataMergeTableStart {
        index: usize,
        total: usize,
        table_name: String,
    },
    DataMergeTableEnd {
        index: usize,
        total: usize,
    },
}

#[derive(Debug)]
enum DuckdbProgress {
    Report(String),
    Event(ProgressEvent),
}

#[derive(Debug, Clone)]
struct DataTableWritePlan {
    table_name: String,
    key_ids: Vec<i64>,
    estimated_values: u128,
}

#[derive(Debug)]
enum DataWriteWorkerEvent {
    TableStarted {
        worker_id: usize,
        index: usize,
        total: usize,
        table_name: String,
        keys: usize,
    },
    TableCompleted {
        worker_id: usize,
        index: usize,
        total: usize,
        table_name: String,
        keys: usize,
    },
}

pub struct DuckdbBuilder<'a> {
    dataset: &'a SolutionDataset,
    db_path: std::path::PathBuf,
    data_write_threads: Option<usize>,
    data_table_name_pattern: Option<regex::Regex>,
    deflate_index_interval_bytes: u64,
    report: Option<&'a mut dyn FnMut(&str)>,
    progress: Option<&'a mut dyn FnMut(ProgressEvent)>,
}

impl<'a> DuckdbBuilder<'a> {
    fn new<P: AsRef<std::path::Path>>(dataset: &'a SolutionDataset, db_path: P) -> Self {
        Self {
            dataset,
            db_path: db_path.as_ref().to_path_buf(),
            data_write_threads: None,
            data_table_name_pattern: None,
            deflate_index_interval_bytes: DEFAULT_DEFLATE_INDEX_INTERVAL_MIB * MIB_BYTES,
            report: None,
            progress: None,
        }
    }

    pub fn with_data_write_threads(mut self, threads: usize) -> Self {
        self.data_write_threads = Some(threads.max(1));
        self
    }

    pub fn with_data_table_name_pattern(mut self, pattern: regex::Regex) -> Self {
        self.data_table_name_pattern = Some(pattern);
        self
    }

    pub fn with_deflate_index_interval_bytes(mut self, bytes: u64) -> Self {
        self.deflate_index_interval_bytes = bytes.max(MIN_DEFLATE_INDEX_INTERVAL_BYTES);
        self
    }

    pub fn with_progress(mut self, report: &'a mut dyn FnMut(&str)) -> Self {
        self.report = Some(report);
        self
    }

    pub fn with_events(mut self, progress: &'a mut dyn FnMut(ProgressEvent)) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn run(mut self) -> Result<()> {
        let mut report = self.report.take();
        let mut progress = self.progress.take();
        let has_callbacks = report.is_some() || progress.is_some();
        let mut combined = |update: DuckdbProgress| match update {
            DuckdbProgress::Report(msg) => {
                if let Some(report) = report.as_mut() {
                    report(msg.as_str());
                }
            },
            DuckdbProgress::Event(event) => {
                if let Some(progress) = progress.as_mut() {
                    progress(event);
                }
            },
        };
        let combined_opt = if has_callbacks {
            Some(&mut combined as &mut dyn FnMut(DuckdbProgress))
        } else {
            None
        };
        self.dataset.to_duckdb_impl(
            &self.db_path,
            combined_opt,
            self.data_write_threads,
            self.data_table_name_pattern.as_ref(),
            self.deflate_index_interval_bytes,
        )
    }
}

impl SolutionDataset {
    fn with_duckdb_step<R>(
        progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
        index: &mut usize,
        total: usize,
        label: &str,
        f: impl FnOnce(&mut Option<&mut dyn FnMut(DuckdbProgress)>) -> Result<R>,
    ) -> Result<R> {
        *index += 1;
        if let Some(report) = progress.as_mut() {
            report(DuckdbProgress::Event(ProgressEvent::DataTableStart {
                index: *index,
                total,
                table_name: label.to_string(),
                keys: 0,
            }));
        }
        let result = f(progress);
        if let Some(report) = progress.as_mut() {
            report(DuckdbProgress::Event(ProgressEvent::DataTableEnd));
        }
        result
    }

    fn report_duckdb_progress(progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>, msg: &str) {
        if let Some(report) = progress.as_mut() {
            report(DuckdbProgress::Report(msg.to_string()));
        }
    }

    fn report_progress(progress: &mut Option<&mut dyn FnMut(&str)>, msg: &str) {
        if let Some(report) = progress.as_mut() {
            report(msg);
        }
    }

    /// Get a unit by its ID
    fn get_unit(&self, id: i64) -> Option<&Unit> {
        self.unit.get(&id)
    }

    /// Get a category by its ID
    fn get_category(&self, id: i64) -> Option<&Category> {
        self.category.get(&id)
    }

    /// Get all categories of a specific class
    fn get_categories_by_class(&self, class_id: i64) -> Vec<&Category> {
        self.category
            .values()
            .filter(|c| c.class_id == class_id)
            .collect()
    }

    pub fn with_model_name(mut self, model_name: String) -> Self {
        if model_name.is_empty() {
            return self;
        }
        self.model_name = model_name;
        self
    }

    pub fn with_period_data(mut self, period_data: indexmap::IndexMap<i64, std::fs::File>) -> Self {
        self.period_data = period_data
            .into_iter()
            .map(|(k, v)| (k, PeriodData::File(v)))
            .collect();
        self
    }

    pub fn with_file<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.file = path.as_ref().to_path_buf();
        self
    }

    pub fn with_simulation_log(mut self, log: String) -> Self {
        if log.is_empty() {
            return self;
        }
        self.simulation_log = Some(log);
        self
    }

    pub fn with_run_stats(mut self, run_stats: String) -> Self {
        if run_stats.is_empty() {
            return self;
        }
        self.run_stats = Some(run_stats);
        self
    }

    fn is_valid_bin_filename(name: &str) -> Option<i64> {
        // Only allow specific pattern: t_data_[digits].BIN
        // Returns the digit if valid, None otherwise
        lazy_static::lazy_static! {
            static ref RE: regex::Regex = regex::Regex::new(r"^t_data_(\d+)\.BIN$").unwrap();
        }

        RE.captures(name)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<i64>().ok())
    }

    pub fn with_zip_file<P: AsRef<std::path::Path>>(self, path: P) -> Result<Self> {
        self.with_zip_file_impl(path, None)
    }

    pub fn with_zip_file_with_progress<P: AsRef<std::path::Path>>(
        self,
        path: P,
        report: &mut dyn FnMut(&str),
    ) -> Result<Self> {
        self.with_zip_file_impl(path, Some(report))
    }

    fn read_zip_archive(
        path: &std::path::Path,
        model_name: &str,
        report: &mut Option<&mut dyn FnMut(&str)>,
    ) -> Result<(String, indexmap::IndexMap<i64, PeriodData>)> {
        Self::report_progress(report, "Opening ZIP archive");
        let archive_file = Arc::new(std::fs::File::open(path)?);

        let zip_stem = path
            .file_stem()
            .ok_or_else(|| eyre!("Invalid zip file name"))?
            .to_string_lossy()
            .to_string();

        let mut archive = zip::ZipArchive::new(archive_file.try_clone()?)?;

        Self::report_progress(report, "Selecting XML inside ZIP archive");
        let mut xml_content = String::new();
        let mut preferred_xml_index = None;
        let mut model_name_xml_index = None;
        let mut first_xml_index = None;

        let model_name = if !model_name.is_empty() {
            Some(model_name.to_lowercase())
        } else {
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            Some(
                file_name
                    .trim_start_matches("Model ")
                    .trim_end_matches(" Solution.zip")
                    .to_lowercase(),
            )
        };

        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let file_name = file.name().to_string();
            let file_name_lower = file_name.to_lowercase();
            if file_name_lower.ends_with(".xml") {
                if let Some(stem) = std::path::Path::new(&file_name)
                    .file_stem()
                    .map(|s| s.to_string_lossy())
                {
                    if stem == zip_stem {
                        preferred_xml_index = Some(i);
                        break;
                    }
                    if model_name
                        .as_ref()
                        .map_or(false, |mn| stem.to_lowercase().contains(mn.as_str()))
                    {
                        if model_name_xml_index.is_none() {
                            model_name_xml_index = Some(i);
                        }
                    }
                }
                if first_xml_index.is_none() {
                    first_xml_index = Some(i);
                }
            }
        }

        let xml_index_to_use = if let Some(idx) = preferred_xml_index {
            idx
        } else if let Some(idx) = model_name_xml_index {
            eprintln!(
                "Warning: Expected XML file named '{}.xml', using XML file containing model name instead.",
                zip_stem
            );
            idx
        } else if let Some(idx) = first_xml_index {
            eprintln!(
                "Warning: Expected XML file named '{}.xml', using first XML file '{}' instead.",
                zip_stem,
                archive.by_index(idx)?.name()
            );
            idx
        } else {
            return Err(eyre!("No XML file found in the zip archive"));
        };

        Self::report_progress(report, "Reading XML from ZIP archive");
        let mut file = archive.by_index(xml_index_to_use)?;
        file.read_to_string(&mut xml_content)?;
        drop(file);

        Self::report_progress(report, "Indexing BIN files");
        let mut period_data = indexmap::IndexMap::new();

        let archive_len = archive.len();
        for i in 0..archive_len {
            let file = archive.by_index(i)?;
            let name = file.name().to_string();

            if let Some(digit) = Self::is_valid_bin_filename(&name) {
                if file.compression() == zip::CompressionMethod::Stored {
                    let data_start = file.data_start().ok_or_else(|| {
                        eyre!("ZIP entry '{}' is missing a data start offset", name)
                    })?;
                    let entry = ZipPeriodData {
                        archive_file: archive_file.clone(),
                        data_start,
                        data_len: file.size(),
                    };
                    period_data.insert(digit, PeriodData::ZipEntry(entry));
                    continue;
                }

                if file.compression() != zip::CompressionMethod::Deflated {
                    return Err(eyre!(
                        "BIN file '{}' uses unsupported ZIP compression method {:?}; only stored and deflated BIN entries can be converted without buffering",
                        name,
                        file.compression()
                    ));
                }

                let data_start = file
                    .data_start()
                    .ok_or_else(|| eyre!("ZIP entry '{}' is missing a data start offset", name))?;
                let entry = CompressedZipPeriodData {
                    archive_file: archive_file.clone(),
                    data_start,
                    compressed_len: file.compressed_size(),
                    data_len: file.size(),
                    name,
                };
                period_data.insert(digit, PeriodData::CompressedZipEntry(entry));
            }
        }

        Ok((xml_content, period_data))
    }

    fn with_zip_file_impl<P: AsRef<std::path::Path>>(
        self,
        path: P,
        mut report: Option<&mut dyn FnMut(&str)>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let (xml_content, period_data) =
            Self::read_zip_archive(path, &self.model_name, &mut report)?;
        Self::report_progress(&mut report, "Parsing XML");
        let mut ds = self
            .with_file(path)
            .with_xml_string_impl(&xml_content, report)?;
        ds.period_data = period_data;
        Ok(ds)
    }

    pub fn with_xml_file<P: AsRef<std::path::Path>>(self, path: P) -> Result<Self> {
        self.with_xml_file_impl(path, None)
    }

    pub fn with_xml_file_with_progress<P: AsRef<std::path::Path>>(
        self,
        path: P,
        report: &mut dyn FnMut(&str),
    ) -> Result<Self> {
        self.with_xml_file_impl(path, Some(report))
    }

    fn with_xml_file_impl<P: AsRef<std::path::Path>>(
        self,
        path: P,
        mut report: Option<&mut dyn FnMut(&str)>,
    ) -> Result<Self> {
        Self::report_progress(&mut report, "Reading XML file");
        let mut file = std::fs::File::open(path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Self::report_progress(&mut report, "Parsing XML");
        self.with_xml_string_impl(&content, report)
    }

    pub fn with_xml_string(self, xml: &str) -> Result<Self> {
        self.with_xml_string_impl(xml, None)
    }

    fn with_xml_string_impl(
        mut self,
        xml: &str,
        mut report: Option<&mut dyn FnMut(&str)>,
    ) -> Result<Self> {
        Self::report_progress(&mut report, "Parsing XML document");
        let doc = Document::parse(xml)?;

        let root = doc.root_element();
        Self::report_progress(&mut report, "Parsing attribute data");
        self.parse_attribute_data(&root)?;
        Self::report_progress(&mut report, "Parsing attributes");
        self.parse_attribute(&root)?;
        Self::report_progress(&mut report, "Parsing properties");
        self.parse_property(&root)?;
        Self::report_progress(&mut report, "Parsing bands");
        self.parse_band(&root)?;
        Self::report_progress(&mut report, "Parsing categories");
        self.parse_category(&root)?;
        Self::report_progress(&mut report, "Parsing class groups");
        self.parse_class_group(&root)?;
        Self::report_progress(&mut report, "Parsing classes");
        self.parse_classes(&root)?;
        Self::report_progress(&mut report, "Parsing collections");
        self.parse_collection(&root)?;
        Self::report_progress(&mut report, "Parsing config");
        self.parse_config(&root)?;
        Self::report_progress(&mut report, "Parsing key indexes");
        self.parse_key_index(&root)?;
        Self::report_progress(&mut report, "Parsing keys");
        self.parse_key(&root)?;
        Self::report_progress(&mut report, "Parsing memberships");
        self.parse_membership(&root)?;
        Self::report_progress(&mut report, "Parsing models");
        self.parse_models(&root)?;
        Self::report_progress(&mut report, "Parsing objects");
        self.parse_object(&root)?;
        Self::report_progress(&mut report, "Parsing period intervals");
        self.parse_period0(&root)?;
        Self::report_progress(&mut report, "Parsing period days");
        self.parse_period1(&root)?;
        Self::report_progress(&mut report, "Parsing period weeks");
        self.parse_period2(&root)?;
        Self::report_progress(&mut report, "Parsing period months");
        self.parse_period3(&root)?;
        Self::report_progress(&mut report, "Parsing period years");
        self.parse_period4(&root)?;
        Self::report_progress(&mut report, "Parsing period hours");
        self.parse_period6(&root)?;
        Self::report_progress(&mut report, "Parsing period quarters");
        self.parse_period7(&root)?;
        Self::report_progress(&mut report, "Parsing phase LT");
        self.parse_phase1(&root)?;
        Self::report_progress(&mut report, "Parsing phase PASA");
        self.parse_phase2(&root)?;
        Self::report_progress(&mut report, "Parsing phase MT");
        self.parse_phase3(&root)?;
        Self::report_progress(&mut report, "Parsing phase ST");
        self.parse_phase4(&root)?;
        Self::report_progress(&mut report, "Parsing samples");
        self.parse_sample(&root)?;
        Self::report_progress(&mut report, "Parsing sample weights");
        self.parse_sample_weight(&root)?;
        Self::report_progress(&mut report, "Parsing timeslices");
        self.parse_timeslice(&root)?;
        Self::report_progress(&mut report, "Parsing units");
        self.parse_unit(&root)?;
        Self::report_progress(&mut report, "Parsing memo objects");
        self.parse_memo_object(&root)?;
        Self::report_progress(&mut report, "Parsing custom columns");
        self.parse_custom_column(&root)?;

        Self::report_progress(&mut report, "Updating property band ids");
        self.update_property_band_id()?;
        Self::report_progress(&mut report, "Building timestamp blocks");
        self.update_timestamp_block()?;
        Self::report_progress(&mut report, "Updating collection membership counts");
        self.update_collection_membership_count()?;
        Self::report_progress(&mut report, "Indexing table key mappings");
        self.update_table_key_indexes_mapping()?;

        Ok(self)
    }

    fn parse_models(&mut self, node: &Node) -> Result<()> {
        for model_node in node.children().filter(|n| n.has_tag_name("t_model")) {
            let model_id = get_child(&model_node, "model_id")?;
            let name = get_child(&model_node, "name")?;

            let model = Model { model_id, name };
            self.model.insert(model.model_id, model);
        }
        self.model.sort_keys();
        Ok(())
    }

    fn parse_object(&mut self, node: &Node) -> Result<()> {
        for object_node in node.children().filter(|n| n.has_tag_name("t_object")) {
            let class_id = get_child(&object_node, "class_id")?;
            let name = get_child(&object_node, "name")?;
            let category_id = get_child(&object_node, "category_id")?;
            let index = get_child(&object_node, "index")?;
            let object_id = get_child(&object_node, "object_id")?;
            let show = get_child(&object_node, "show")?;
            let guid = get_child(&object_node, "GUID").ok();

            let object = Object {
                class_id,
                name,
                category_id,
                index,
                object_id,
                show,
                guid,
            };
            self.object.insert(object.object_id, object);
        }
        self.object.sort_keys();
        Ok(())
    }

    fn parse_membership(&mut self, node: &Node) -> Result<()> {
        for membership_node in node.children().filter(|n| n.has_tag_name("t_membership")) {
            let membership_id = get_child(&membership_node, "membership_id")?;
            let parent_class_id = get_child(&membership_node, "parent_class_id")?;
            let child_class_id = get_child(&membership_node, "child_class_id")?;
            let collection_id = get_child(&membership_node, "collection_id")?;
            let parent_object_id = get_child(&membership_node, "parent_object_id")?;
            let child_object_id = get_child(&membership_node, "child_object_id")?;

            let membership = Membership {
                membership_id,
                parent_class_id,
                child_class_id,
                collection_id,
                parent_object_id,
                child_object_id,
                collection_idx: 0,
            };
            self.membership.insert(membership.membership_id, membership);
        }
        self.membership.sort_keys();
        Ok(())
    }

    fn parse_attribute(&mut self, node: &Node) -> Result<()> {
        for attribute_node in node.children().filter(|n| n.has_tag_name("t_attribute")) {
            let attribute_id = get_child(&attribute_node, "attribute_id")?;
            let class_id = get_child(&attribute_node, "class_id")?;
            let enum_id = get_child(&attribute_node, "enum_id")?;
            let name = get_child(&attribute_node, "name")?;
            let description = get_child(&attribute_node, "description")?;
            let input_mask = get_child(&attribute_node, "input_mask").ok();
            let lang_id = get_child(&attribute_node, "lang_id")?;

            let attribute = Attribute {
                attribute_id,
                class_id,
                enum_id,
                name,
                description,
                lang_id,
                input_mask,
            };
            self.attribute.insert(attribute.attribute_id, attribute);
        }
        self.attribute.sort_keys();
        Ok(())
    }

    fn parse_property(&mut self, node: &Node) -> Result<()> {
        for property_node in node.children().filter(|n| n.has_tag_name("t_property")) {
            let property_id = get_child(&property_node, "property_id")?;
            let name = get_child(&property_node, "name")?;
            let summary_name = get_child(&property_node, "summary_name")?;
            let enum_id = get_child(&property_node, "enum_id")?;
            let unit_id = get_child(&property_node, "unit_id")?;
            let summary_unit_id = get_child(&property_node, "summary_unit_id")?;
            let is_multi_band = get_child(&property_node, "is_multi_band")?;
            let is_period = get_child(&property_node, "is_period")?;
            let is_summary = get_child(&property_node, "is_summary")?;
            let collection_id = get_child(&property_node, "collection_id")?;
            let lang_id = get_child(&property_node, "lang_id")?;

            let property = Property {
                property_id,
                name,
                summary_name,
                lang_id,
                enum_id,
                unit_id,
                summary_unit_id,
                is_multi_band,
                is_period,
                is_summary,
                collection_id,
                band_id: 0,
            };
            self.property.insert(property.property_id, property);
        }
        self.property.sort_keys();
        Ok(())
    }

    fn parse_config(&mut self, node: &Node) -> Result<()> {
        for config_node in node.children().filter(|n| n.has_tag_name("t_config")) {
            let element = get_child(&config_node, "element")?;
            let value = get_child(&config_node, "value").ok();

            self.config.insert(element, value);
        }
        self.config.sort_keys();
        Ok(())
    }

    fn parse_unit(&mut self, node: &Node) -> Result<()> {
        for node in node.children().filter(|n| n.has_tag_name("t_unit")) {
            let unit_id = get_child(&node, "unit_id")?;
            let value = get_child(&node, "value")?;
            let lang_id = get_child(&node, "lang_id")?;

            let unit = Unit {
                id: unit_id,
                value,
                lang_id,
            };

            self.unit.insert(unit.id, unit);
        }
        self.unit.sort_keys();
        Ok(())
    }

    fn parse_band(&mut self, node: &Node) -> Result<()> {
        for node in node.children().filter(|n| n.has_tag_name("t_band")) {
            let band_id = get_child(&node, "band_id")?;
            self.band.insert(band_id, band_id);
        }
        self.band.sort_keys();
        Ok(())
    }

    fn parse_category(&mut self, node: &Node) -> Result<()> {
        for node in node.children().filter(|n| n.has_tag_name("t_category")) {
            let category_id = get_child(&node, "category_id")?;
            let class_id = get_child(&node, "class_id")?;
            let rank = get_child(&node, "rank")?;
            let name = get_child(&node, "name")?;

            let category = Category {
                category_id,
                class_id,
                rank,
                name,
            };
            self.category.insert(category.category_id, category);
        }
        self.category.sort_keys();
        Ok(())
    }

    fn parse_classes(&mut self, node: &Node) -> Result<()> {
        for class_node in node.children().filter(|n| n.has_tag_name("t_class")) {
            let class_id = get_child(&class_node, "class_id")?;
            let name = get_child(&class_node, "name")?;
            let class_group_id = get_child(&class_node, "class_group_id")?;
            let lang_id = get_child(&class_node, "lang_id")?;
            let state = get_child(&class_node, "state").ok();

            let class = Class {
                class_id,
                name,
                class_group_id,
                lang_id,
                state,
            };
            self.class.insert(class.class_id, class);
        }
        self.class.sort_keys();
        Ok(())
    }

    fn parse_class_group(&mut self, node: &Node) -> Result<()> {
        for class_group_node in node.children().filter(|n| n.has_tag_name("t_class_group")) {
            let class_group_id = get_child(&class_group_node, "class_group_id")?;
            let name = get_child(&class_group_node, "name")?;
            let lang_id = get_child(&class_group_node, "lang_id")?;
            let state = get_child(&class_group_node, "state").ok();

            let class_group = ClassGroup {
                class_group_id,
                name,
                lang_id,
                state,
            };
            self.class_group
                .insert(class_group.class_group_id, class_group);
        }
        self.class_group.sort_keys();
        Ok(())
    }

    fn parse_collection(&mut self, node: &Node) -> Result<()> {
        for collection_node in node.children().filter(|n| n.has_tag_name("t_collection")) {
            let collection_id = get_child(&collection_node, "collection_id")?;
            let parent_class_id = get_child(&collection_node, "parent_class_id")?;
            let child_class_id = get_child(&collection_node, "child_class_id")?;
            let name = get_child(&collection_node, "name")?;
            let complement_name = get_child(&collection_node, "complement_name").ok();
            let lang_id = get_child(&collection_node, "lang_id")?;

            let collection = Collection {
                collection_id,
                parent_class_id,
                child_class_id,
                name,
                complement_name,
                lang_id,
                n_members: 0,
            };
            self.collection.insert(collection.collection_id, collection);
        }
        self.collection.sort_keys();
        Ok(())
    }

    fn parse_key(&mut self, node: &Node) -> Result<()> {
        for key_node in node.children().filter(|n| n.has_tag_name("t_key")) {
            let key_id = get_child(&key_node, "key_id")?;
            let membership_id = get_child(&key_node, "membership_id")?;
            let model_id = get_child(&key_node, "model_id")?;
            let phase_id = get_child(&key_node, "phase_id")?;
            let property_id = get_child(&key_node, "property_id")?;
            // period_type_id is 0 or 1
            // 1 for summary keys and 0 for non-summary keys
            let period_type_id: i64 = get_child(&key_node, "period_type_id")?;
            let band_id = get_child(&key_node, "band_id")?;
            let sample_id = get_child(&key_node, "sample_id")?;
            let timeslice_id = get_child(&key_node, "timeslice_id")?;

            let key = Key {
                key_id,
                membership_id,
                model_id,
                phase_id,
                property_id,
                is_summary: period_type_id == 1,
                band_id,
                sample_id,
                timeslice_id,
            };
            self.key.insert(key.key_id, key);
        }

        self.key.sort_keys();

        Ok(())
    }

    fn parse_key_index(&mut self, node: &Node) -> Result<()> {
        for key_index_node in node.children().filter(|n| n.has_tag_name("t_key_index")) {
            let key_id = get_child(&key_index_node, "key_id")?;
            let period_type_id = get_child(&key_index_node, "period_type_id")?;
            let position = get_child(&key_index_node, "position")?;
            let length = get_child(&key_index_node, "length")?;
            let period_offset = get_child(&key_index_node, "period_offset")?;

            let key_index = KeyIndex {
                key_id,
                period_type_id,
                position,
                length,
                period_offset,
            };
            self.key_index.insert(key_index.key_id, key_index);
        }
        self.key_index.sort_keys();

        Ok(())
    }

    fn parse_period0(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_0")) {
            let interval_id = get_child(&period_node, "interval_id")?;
            let hour_id = get_child(&period_node, "hour_id")?;
            let day_id = get_child(&period_node, "day_id")?;
            let week_id = get_child(&period_node, "week_id")?;
            let month_id = get_child(&period_node, "month_id")?;
            let fiscal_year_id = get_child(&period_node, "fiscal_year_id")?;
            let datetime: String = get_child(&period_node, "datetime")?;
            let datetime = chrono::DateTime::parse_from_str(
                &format!("{datetime} +0000"),
                "%d/%m/%Y %H:%M:%S %z",
            )?
            .into();
            let period_of_day = get_child(&period_node, "period_of_day")?;
            let quarter_id = get_child(&period_node, "quarter_id").ok();

            let period0 = Period0 {
                interval_id,
                hour_id,
                day_id,
                week_id,
                month_id,
                fiscal_year_id,
                datetime,
                period_of_day,
                quarter_id,
            };
            self.period
                .entry("interval".to_string())
                .or_default()
                .insert(period0.interval_id, PeriodType::Interval(period0));
        }
        self.period
            .entry("interval".to_string())
            .or_default()
            .sort_keys();
        Ok(())
    }

    fn parse_period1(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_1")) {
            let day_id = get_child(&period_node, "day_id")?;
            let date: String = get_child(&period_node, "date")?;
            let date = parse_datetime_to_utc(&date)?;
            let week_id = get_child(&period_node, "week_id")?;
            let month_id = get_child(&period_node, "month_id")?;
            let fiscal_year_id = get_child(&period_node, "fiscal_year_id")?;
            let quarter_id = get_child(&period_node, "quarter_id").ok();
            let period1 = Period1 {
                day_id,
                date,
                week_id,
                month_id,
                fiscal_year_id,
                quarter_id,
            };

            self.period
                .entry("day".to_string())
                .or_default()
                .insert(period1.day_id, PeriodType::Day(period1));
        }
        self.period
            .entry("day".to_string())
            .or_default()
            .sort_keys();
        Ok(())
    }

    fn parse_period2(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_2")) {
            let week_id = get_child(&period_node, "week_id")?;
            let week_ending: String = get_child(&period_node, "week_ending")?;
            let week_ending = parse_datetime_to_utc(&week_ending)?;
            let period2 = Period2 {
                week_id,
                week_ending,
            };
            self.period
                .entry("week".to_string())
                .or_default()
                .insert(period2.week_id, PeriodType::Week(period2));
        }
        self.period
            .entry("week".to_string())
            .or_default()
            .sort_keys();
        Ok(())
    }

    fn parse_period3(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_3")) {
            let month_id = get_child(&period_node, "month_id")?;
            let month_beginning: String = get_child(&period_node, "month_beginning")?;
            let month_beginning = parse_datetime_to_utc(&month_beginning)?;
            let period3 = Period3 {
                month_id,
                month_beginning,
            };
            self.period
                .entry("month".to_string())
                .or_default()
                .insert(period3.month_id, PeriodType::Month(period3));
        }
        self.period
            .entry("month".to_string())
            .or_default()
            .sort_keys();
        Ok(())
    }

    fn parse_period4(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_4")) {
            let fiscal_year_id = get_child(&period_node, "fiscal_year_id")?;
            let year_ending: String = get_child(&period_node, "year_ending")?;
            let year_ending = parse_datetime_to_utc(&year_ending)?;
            let period4 = Period4 {
                fiscal_year_id,
                year_ending,
            };
            self.period
                .entry("year".to_string())
                .or_default()
                .insert(period4.fiscal_year_id, PeriodType::Year(period4));
        }
        self.period
            .entry("year".to_string())
            .or_default()
            .sort_keys();
        Ok(())
    }

    fn parse_period6(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_6")) {
            let hour_id = get_child(&period_node, "hour_id")?;
            let datetime: String = get_child(&period_node, "datetime")?;
            let datetime = parse_datetime_to_utc(&datetime)?;
            let period6 = Period6 { hour_id, datetime };
            self.period
                .entry("hour".to_string())
                .or_default()
                .insert(period6.hour_id, PeriodType::Hour(period6));
        }
        self.period
            .entry("hour".to_string())
            .or_default()
            .sort_keys();
        Ok(())
    }

    fn parse_period7(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_7")) {
            let quarter_id = get_child(&period_node, "quarter_id")?;
            let quarter_beginning: String = get_child(&period_node, "quarter_beginning")?;
            let quarter_beginning = parse_datetime_to_utc(&quarter_beginning)?;
            let period7 = Period7 {
                quarter_id,
                quarter_beginning,
            };
            self.period
                .entry("quarter".to_string())
                .or_default()
                .insert(period7.quarter_id, PeriodType::Quarter(period7));
        }
        self.period
            .entry("quarter".to_string())
            .or_default()
            .sort_keys();
        Ok(())
    }

    fn parse_phase1(&mut self, node: &Node) -> Result<()> {
        for phase_node in node.children().filter(|n| n.has_tag_name("t_phase_1")) {
            let interval_id = get_child(&phase_node, "interval_id")?;
            let period_id = get_child(&phase_node, "period_id")?;

            let phase1 = Phase1 {
                interval_id,
                period_id,
            };
            self.phase
                .entry("LT".to_string())
                .or_default()
                .insert(phase1.interval_id, PhaseType::LT(phase1));
        }
        self.phase.entry("LT".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_phase2(&mut self, node: &Node) -> Result<()> {
        for phase_node in node.children().filter(|n| n.has_tag_name("t_phase_2")) {
            let interval_id = get_child(&phase_node, "interval_id")?;
            let period_id = get_child(&phase_node, "period_id")?;

            let phase2 = Phase2 {
                interval_id,
                period_id,
            };
            self.phase
                .entry("PASA".to_string())
                .or_default()
                .insert(phase2.interval_id, PhaseType::PASA(phase2));
        }
        self.phase
            .entry("PASA".to_string())
            .or_default()
            .sort_keys();
        Ok(())
    }

    fn parse_phase3(&mut self, node: &Node) -> Result<()> {
        for phase_node in node.children().filter(|n| n.has_tag_name("t_phase_3")) {
            let interval_id = get_child(&phase_node, "interval_id")?;
            let period_id = get_child(&phase_node, "period_id")?;

            let phase3 = Phase3 {
                interval_id,
                period_id,
            };
            self.phase
                .entry("MT".to_string())
                .or_default()
                .insert(phase3.interval_id, PhaseType::MT(phase3));
        }
        self.phase.entry("MT".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_phase4(&mut self, node: &Node) -> Result<()> {
        for phase_node in node.children().filter(|n| n.has_tag_name("t_phase_4")) {
            let interval_id = get_child(&phase_node, "interval_id")?;
            let period_id = get_child(&phase_node, "period_id")?;

            let phase4 = Phase4 {
                interval_id,
                period_id,
            };
            self.phase
                .entry("ST".to_string())
                .or_default()
                .insert(phase4.interval_id, PhaseType::ST(phase4));
        }
        self.phase.entry("ST".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_sample(&mut self, node: &Node) -> Result<()> {
        for sample_node in node.children().filter(|n| n.has_tag_name("t_sample")) {
            let id = get_child(&sample_node, "sample_id")?;
            let name = get_child(&sample_node, "sample_name").ok();

            let sample = Sample {
                sample_id: id,
                name,
            };
            self.sample.insert(sample.sample_id, sample);
        }
        self.sample.sort_keys();
        Ok(())
    }

    fn parse_sample_weight(&mut self, node: &Node) -> Result<()> {
        for sample_weight_node in node
            .children()
            .filter(|n| n.has_tag_name("t_sample_weight"))
        {
            let sample_id = get_child(&sample_weight_node, "sample_id")?;
            let phase_id = get_child(&sample_weight_node, "phase_id")?;
            let weight = get_child(&sample_weight_node, "value")?;

            let sample_weight = SampleWeight {
                sample_id,
                phase_id,
                weight,
            };
            self.sample_weight
                .insert(sample_weight.sample_id, sample_weight);
        }
        self.sample_weight.sort_keys();
        Ok(())
    }

    fn parse_timeslice(&mut self, node: &Node) -> Result<()> {
        for timeslice_node in node.children().filter(|n| n.has_tag_name("t_timeslice")) {
            let timeslice_id = get_child(&timeslice_node, "timeslice_id")?;
            let name = get_child(&timeslice_node, "name")?;

            let timeslice = Timeslice { timeslice_id, name };
            self.timeslice.insert(timeslice.timeslice_id, timeslice);
        }
        self.timeslice.sort_keys();
        Ok(())
    }

    fn parse_attribute_data(&mut self, node: &Node) -> Result<()> {
        for attribute_node in node
            .children()
            .filter(|n| n.has_tag_name("t_attribute_data"))
        {
            let object_id = get_child(&attribute_node, "object_id").ok();
            let attribute_id = get_child(&attribute_node, "attribute_id")?;
            let value = get_child(&attribute_node, "value")?;

            let attribute_data = AttributeData {
                object_id,
                attribute_id,
                value,
            };
            self.attribute_data
                .insert(attribute_data.attribute_id, attribute_data);
        }
        self.attribute_data.sort_keys();
        Ok(())
    }

    fn parse_memo_object(&mut self, node: &Node) -> Result<()> {
        for memo_node in node.children().filter(|n| n.has_tag_name("t_memo_object")) {
            let value = get_child(&memo_node, "value")?;
            let column_id = get_child(&memo_node, "column_id")?;
            let object_id = get_child(&memo_node, "object_id")?;

            let memo_object = MemoObject {
                value,
                column_id,
                object_id,
            };
            self.memo_object.push(memo_object);
        }

        Ok(())
    }

    fn parse_custom_column(&mut self, node: &Node) -> Result<()> {
        for custom_column_node in node
            .children()
            .filter(|n| n.has_tag_name("t_custom_column"))
        {
            let column_id = get_child(&custom_column_node, "column_id")?;
            let name = get_child(&custom_column_node, "name")?;
            let position = get_child(&custom_column_node, "position")?;
            let class_id = get_child(&custom_column_node, "class_id")?;
            let custom_column = CustomColumn {
                column_id,
                name,
                position,
                class_id,
            };
            self.custom_column
                .insert(custom_column.column_id, custom_column);
        }
        self.custom_column.sort_keys();
        Ok(())
    }

    fn update_property_band_id(&mut self) -> Result<()> {
        for (_, key) in self.key.iter() {
            let property_id = key.property_id;
            let band_id = key.band_id;
            if let Some(p) = self.property.get_mut(&property_id) {
                p.band_id = (p.band_id).max(band_id);
            }
        }
        Ok(())
    }

    fn update_timestamp_block(&mut self) -> Result<()> {
        let extractors: Vec<(&str, fn(&Self, i64) -> Result<PeriodType>)> = vec![
            ("Interval", Self::interval),
            ("Day", Self::day),
            ("Week", Self::week),
            ("Month", Self::month),
            ("Year", Self::year),
            ("Hour", Self::hour),
            ("Quarter", Self::quarter),
        ];
        for phase_indexmap in self.phase.values() {
            for phase_type in phase_indexmap.values() {
                let phase_name = phase_type.name();
                let interval_id = phase_type.interval_id();
                let period_id = phase_type.period_id();

                for (period_name, extractor) in &extractors {
                    if let Ok(period) = extractor(self, interval_id) {
                        let datetime = period.datetime();
                        let key = format!("{}__{}", phase_name, period_name);
                        self.timestamp_block
                            .entry(key)
                            .or_default()
                            .push((datetime, period_id));
                    }
                }
            }
        }

        Ok(())
    }

    fn update_collection_membership_count(&mut self) -> Result<()> {
        // count how many memberships per collection
        let mut membership_counts: std::collections::HashMap<i64, usize> =
            std::collections::HashMap::new();

        for membership in self.membership.values() {
            *membership_counts
                .entry(membership.collection_id)
                .or_insert(0) += 1;
        }

        // assign 0-based index per membership and update collection's n_members
        let mut collection_indices: std::collections::HashMap<i64, usize> =
            std::collections::HashMap::new();

        for membership in self.membership.values_mut() {
            let collection_id = membership.collection_id;

            let idx = collection_indices.entry(collection_id).or_insert(0);
            membership.collection_idx = *idx;
            *idx += 1;

            if let Some(collection) = self.collection.get_mut(&collection_id) {
                collection.n_members = *membership_counts.get(&collection_id).unwrap_or(&0);
            }
        }

        Ok(())
    }

    fn update_table_key_indexes_mapping(&mut self) -> Result<()> {
        let mut key_indexes_mapping: std::collections::HashMap<String, Vec<i64>> =
            Default::default();
        let mut units_mapping: std::collections::HashMap<String, (String, i64)> =
            Default::default();

        for ki in self.key_index.values() {
            let key_id = ki.key_id;
            let key = self.key(key_id)?;

            let phase_id = key.phase_id;
            let period_type_id = ki.period_type_id;
            let phase_name = self.phase_name(phase_id);
            let period_name = self.period_name(period_type_id);
            let membership = self.membership(key.membership_id)?;
            let collection = self.collection(membership.collection_id)?;
            let property = self.property(key.property_id)?;
            let collection_name = collection.name.clone();
            let property_name = if key.is_summary {
                property.summary_name()
            } else {
                property.property_name()
            };
            let unit_id = if property.is_summary {
                property.summary_unit_id
            } else {
                property.unit_id
            };
            let unit = self.unit(unit_id)?;
            let unit_name = unit.value.clone();
            let period_offset = ki.period_offset;

            let table_name =
                format!("{phase_name}__{period_name}__{collection_name}__{property_name}")
                    .replace(" ", "_")
                    .replace("-", "_");

            key_indexes_mapping
                .entry(table_name.clone())
                .or_default()
                .push(key_id);
            units_mapping.insert(table_name, (unit_name, period_offset));
        }
        self.table_key_index_mapping = key_indexes_mapping;
        self.table_units_mapping = units_mapping;
        Ok(())
    }

    pub fn to_duckdb<P: AsRef<std::path::Path>>(&self, db_path: P) -> DuckdbBuilder<'_> {
        DuckdbBuilder::new(self, db_path)
    }

    fn to_duckdb_impl<P: AsRef<std::path::Path>>(
        &self,
        db_path: P,
        mut progress: Option<&mut dyn FnMut(DuckdbProgress)>,
        data_write_threads: Option<usize>,
        data_table_name_pattern: Option<&regex::Regex>,
        deflate_index_interval_bytes: u64,
    ) -> Result<()> {
        let db_path = db_path.as_ref();
        let total_steps = 28;
        let mut step_index = 0;
        let mut direct_stage_dir = None;
        Self::report_duckdb_progress(&mut progress, "Initializing DuckDB");
        let mut con = Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Initializing DuckDB",
            |_progress| {
                let (con, stage_dir) = Self::open_duckdb_write_connection(db_path)?;
                direct_stage_dir = Some(stage_dir);
                Ok(con)
            },
        )?;

        Self::report_duckdb_progress(&mut progress, "Configuring DuckDB session");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Configuring DuckDB session",
            |_progress| {
                con.execute_batch("SET preserve_insertion_order = false;")?;
                Ok(())
            },
        )?;

        Self::report_duckdb_progress(&mut progress, "Creating raw schema");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Creating raw schema",
            |_progress| {
                con.execute_batch("CREATE SCHEMA IF NOT EXISTS raw;")?;
                Ok(())
            },
        )?;

        Self::report_duckdb_progress(&mut progress, "Writing metadata");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing metadata",
            |progress| self.populate_table_metadata(&mut con, progress),
        )?;

        Self::report_duckdb_progress(&mut progress, "Writing config");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing config",
            |progress| self.populate_table_config(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing memberships");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing memberships",
            |progress| self.populate_table_memberships(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing collections");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing collections",
            |progress| self.populate_table_collections(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing classes");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing classes",
            |progress| self.populate_table_classes(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing class groups");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing class groups",
            |progress| self.populate_table_class_groups(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing categories");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing categories",
            |progress| self.populate_table_categories(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing bands");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing bands",
            |progress| self.populate_table_bands(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing models");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing models",
            |progress| self.populate_table_models(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing objects");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing objects",
            |progress| self.populate_table_objects(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing keys");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing keys",
            |progress| self.populate_table_keys(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing key indexes");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing key indexes",
            |progress| self.populate_table_key_indexes(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing properties");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing properties",
            |progress| self.populate_table_properties(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing timeslices");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing timeslices",
            |progress| self.populate_table_timeslices(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing samples");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing samples",
            |progress| self.populate_table_samples(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing units");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing units",
            |progress| self.populate_table_units(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing memo objects");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing memo objects",
            |progress| self.populate_table_memo_objects(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing custom columns");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing custom columns",
            |progress| self.populate_table_custom_columns(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing attribute data");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing attribute data",
            |progress| self.populate_table_attribute_data(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing attributes");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing attributes",
            |progress| self.populate_table_attributes(&mut con, progress),
        )?;
        Self::report_duckdb_progress(&mut progress, "Writing timestamp blocks");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing timestamp blocks",
            |progress| self.populate_table_timestamps_block(&mut con, progress),
        )?;

        Self::report_duckdb_progress(&mut progress, "Writing time series data");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Writing time series data",
            |progress| {
                self.populate_table_data(
                    &mut con,
                    progress,
                    data_write_threads,
                    data_table_name_pattern,
                    db_path,
                    deflate_index_interval_bytes,
                )
            },
        )?;

        Self::report_duckdb_progress(&mut progress, "Creating processed views");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Creating processed views",
            |_progress| {
                self.create_processed_views(&mut con)?;
                Ok(())
            },
        )?;

        Self::report_duckdb_progress(&mut progress, "Creating report views");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Creating report views",
            |_progress| {
                self.create_report_views(&mut con, data_table_name_pattern)?;
                Ok(())
            },
        )?;

        Self::report_duckdb_progress(&mut progress, "Persisting DuckDB database");
        Self::with_duckdb_step(
            &mut progress,
            &mut step_index,
            total_steps,
            "Persisting DuckDB database",
            |_progress| Self::persist_duckdb_database(&mut con, db_path),
        )?;

        drop(con);
        drop(direct_stage_dir);
        Ok(())
    }

    fn populate_table_data(
        &self,
        con: &mut duckdb::Connection,
        progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
        data_write_threads: Option<usize>,
        data_table_name_pattern: Option<&regex::Regex>,
        db_path: &std::path::Path,
        deflate_index_interval_bytes: u64,
    ) -> Result<()> {
        con.execute_batch("CREATE SCHEMA IF NOT EXISTS data;")?;

        let plans = self.build_data_table_plans(data_table_name_pattern)?;
        let total_tables = plans.len();
        if total_tables == 0 {
            return Ok(());
        }

        for plan in &plans {
            self.create_data_table(con, plan.table_name.as_str())?;
        }

        let staging_parent = Self::duckdb_staging_parent(db_path);
        if self.has_compressed_period_data() {
            let range_worker_count = Self::resolve_data_range_write_threads(data_write_threads);
            return self.populate_table_data_indexed_ranges(
                con,
                plans,
                range_worker_count,
                staging_parent.as_path(),
                deflate_index_interval_bytes,
                progress,
            );
        }

        let worker_count = Self::resolve_data_write_threads(total_tables, data_write_threads);
        self.populate_table_data_uncompressed_parquet(
            con,
            plans,
            worker_count,
            staging_parent.as_path(),
            progress,
        )
    }

    fn data_table_name_matches(
        table_name: &str,
        data_table_name_pattern: Option<&regex::Regex>,
    ) -> bool {
        match data_table_name_pattern {
            Some(pattern) => pattern.is_match(table_name),
            None => true,
        }
    }

    fn build_data_table_plans(
        &self,
        data_table_name_pattern: Option<&regex::Regex>,
    ) -> Result<Vec<DataTableWritePlan>> {
        let mut plans = Vec::with_capacity(self.table_key_index_mapping.len());
        for (table_name, key_ids) in self.table_key_index_mapping.iter() {
            if !Self::data_table_name_matches(table_name, data_table_name_pattern) {
                continue;
            }

            let mut estimated_values = 0u128;
            for key_id in key_ids.iter().copied() {
                let length = self.key_index(key_id)?.length;
                estimated_values = estimated_values
                    .checked_add(u128::from(length))
                    .ok_or_else(|| {
                        eyre!(
                            "Estimated workload overflow for table '{}' while planning data writes",
                            table_name
                        )
                    })?;
            }

            plans.push(DataTableWritePlan {
                table_name: table_name.clone(),
                key_ids: key_ids.clone(),
                estimated_values,
            });
        }

        plans.sort_by(|a, b| {
            b.estimated_values
                .cmp(&a.estimated_values)
                .then_with(|| b.key_ids.len().cmp(&a.key_ids.len()))
                .then_with(|| a.table_name.cmp(&b.table_name))
        });
        Ok(plans)
    }

    fn resolve_data_write_threads(total_tables: usize, configured_threads: Option<usize>) -> usize {
        const MAX_AUTO_DATA_WRITE_THREADS: usize = 8;

        match configured_threads {
            Some(threads) => threads.max(1).min(total_tables),
            None => std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .min(MAX_AUTO_DATA_WRITE_THREADS)
                .min(total_tables),
        }
    }

    fn resolve_data_range_write_threads(configured_threads: Option<usize>) -> usize {
        const MAX_AUTO_DATA_RANGE_WRITE_THREADS: usize = 16;

        match configured_threads {
            Some(threads) => threads.max(1),
            None => std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .min(MAX_AUTO_DATA_RANGE_WRITE_THREADS),
        }
    }

    fn has_compressed_period_data(&self) -> bool {
        self.period_data
            .values()
            .any(PeriodData::is_compressed_zip_entry)
    }

    fn populate_table_data_uncompressed_parquet(
        &self,
        con: &mut duckdb::Connection,
        plans: Vec<DataTableWritePlan>,
        worker_count: usize,
        staging_parent: &std::path::Path,
        progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        let total_tables = plans.len();
        let worker_plans = Self::distribute_data_table_plans(plans, worker_count);
        Self::report_duckdb_progress(
            progress,
            &format!(
                "Writing uncompressed BIN tables with {} workers",
                worker_plans.len()
            ),
        );

        let staging_dir = tempfile::Builder::new()
            .prefix("plexos2duckdb-data-parquet-")
            .tempdir_in(staging_parent)?;
        let (tx, rx) = std::sync::mpsc::channel::<DataWriteWorkerEvent>();
        let staged_files = std::thread::scope(|scope| -> Result<Vec<StagedDataFiles>> {
            let mut handles = Vec::with_capacity(worker_plans.len());
            for (worker_idx, worker_plan) in worker_plans.into_iter().enumerate() {
                let worker_dir = staging_dir.path().join(format!("data_worker_{worker_idx}"));
                let worker_tx = tx.clone();

                handles.push(scope.spawn(move || -> Result<StagedDataFiles> {
                    std::fs::create_dir_all(&worker_dir)?;
                    let table_files = self.write_data_table_plans_to_parquet_files(
                        worker_idx,
                        worker_plan,
                        &worker_dir,
                        &worker_tx,
                    )?;
                    Ok(StagedDataFiles::new(table_files))
                }));
            }
            drop(tx);

            let mut completed_tables = 0usize;
            while completed_tables < total_tables {
                let event = match rx.recv() {
                    Ok(event) => event,
                    Err(_) => {
                        for (worker_idx, handle) in handles.into_iter().enumerate() {
                            let result = handle
                                .join()
                                .map_err(|_| eyre!("Data worker {} panicked", worker_idx + 1))?;
                            if let Err(err) = result {
                                return Err(eyre!("Data worker {} failed: {err}", worker_idx + 1));
                            }
                        }
                        return Err(eyre!(
                            "Worker progress channel closed before all tables completed ({}/{})",
                            completed_tables,
                            total_tables
                        ));
                    },
                };
                match event {
                    DataWriteWorkerEvent::TableStarted {
                        worker_id,
                        index,
                        total,
                        table_name,
                        keys,
                    } => {
                        if let Some(report) = progress.as_mut() {
                            report(DuckdbProgress::Event(ProgressEvent::DataWorkerTableStart {
                                worker_id,
                                index,
                                total,
                                table_name,
                                keys,
                            }));
                        }
                    },
                    DataWriteWorkerEvent::TableCompleted {
                        worker_id,
                        index,
                        total,
                        table_name,
                        keys,
                    } => {
                        if let Some(report) = progress.as_mut() {
                            report(DuckdbProgress::Event(ProgressEvent::DataWorkerTableEnd {
                                worker_id,
                                index,
                                total,
                            }));
                        }

                        completed_tables += 1;
                        if let Some(report) = progress.as_mut() {
                            report(DuckdbProgress::Event(ProgressEvent::DataTableStart {
                                index: completed_tables,
                                total: total_tables,
                                table_name,
                                keys,
                            }));
                            report(DuckdbProgress::Event(ProgressEvent::DataTableEnd));
                        }
                    },
                }
            }

            let mut staged_files = Vec::with_capacity(handles.len());
            Self::report_duckdb_progress(progress, "Finalizing staged parquet files");
            for (worker_idx, handle) in handles.into_iter().enumerate() {
                let result = handle
                    .join()
                    .map_err(|_| eyre!("Data worker {} panicked", worker_idx + 1))?;
                staged_files.push(
                    result.map_err(|err| eyre!("Data worker {} failed: {err}", worker_idx + 1))?,
                );
            }
            Ok(staged_files)
        })?;

        Self::report_duckdb_progress(progress, "Merging staged parquet files");
        self.merge_staged_data_files(con, &staged_files, progress)?;
        Ok(())
    }

    fn distribute_data_table_plans(
        plans: Vec<DataTableWritePlan>,
        worker_count: usize,
    ) -> Vec<Vec<DataTableWritePlan>> {
        let mut worker_plans = vec![Vec::<DataTableWritePlan>::new(); worker_count];
        let mut worker_loads = vec![0u128; worker_count];

        for plan in plans {
            let (worker_idx, _) = worker_loads
                .iter()
                .enumerate()
                .min_by_key(|(_, load)| **load)
                .expect("worker_count must be non-zero");

            worker_loads[worker_idx] =
                worker_loads[worker_idx].saturating_add(plan.estimated_values);
            worker_plans[worker_idx].push(plan);
        }

        worker_plans.retain(|p| !p.is_empty());
        worker_plans
    }

    fn write_data_table_plans_to_parquet_files(
        &self,
        worker_idx: usize,
        worker_plan: Vec<DataTableWritePlan>,
        worker_dir: &std::path::Path,
        worker_tx: &std::sync::mpsc::Sender<DataWriteWorkerEvent>,
    ) -> Result<std::collections::BTreeMap<String, Vec<std::path::PathBuf>>> {
        let worker_total = worker_plan.len();
        let mut table_files = std::collections::BTreeMap::<String, Vec<std::path::PathBuf>>::new();

        for (worker_table_idx, table_plan) in worker_plan.into_iter().enumerate() {
            let worker_table_index = worker_table_idx + 1;
            let table_name = table_plan.table_name.clone();
            let keys = table_plan.key_ids.len();
            let _ = worker_tx.send(DataWriteWorkerEvent::TableStarted {
                worker_id: worker_idx,
                index: worker_table_index,
                total: worker_total,
                table_name: table_name.clone(),
                keys,
            });

            let parquet_path = worker_dir.join(format!("table_{worker_table_index:05}.parquet"));
            let mut writer = Self::open_data_parquet_writer(&parquet_path)?;
            self.write_data_table_to_parquet(&mut writer, &table_plan)?;
            writer.close()?;

            table_files
                .entry(table_name.clone())
                .or_default()
                .push(parquet_path);

            let _ = worker_tx.send(DataWriteWorkerEvent::TableCompleted {
                worker_id: worker_idx,
                index: worker_table_index,
                total: worker_total,
                table_name,
                keys,
            });
        }

        Ok(table_files)
    }

    fn write_data_table_to_parquet(
        &self,
        writer: &mut ArrowWriter<std::fs::File>,
        plan: &DataTableWritePlan,
    ) -> Result<()> {
        for key_id in plan.key_ids.iter().copied() {
            let ki = self.key_index(key_id)?;
            let key = self.key(key_id)?;

            if ki.position % 8 != 0 {
                return Err(eyre!(
                    "BIN position misaligned for key_id {} (pos_bytes={})",
                    key_id,
                    ki.position
                ));
            }

            let task = DataRangeWriteTask {
                table_name: plan.table_name.clone(),
                key_id,
                sample_id: key.sample_id,
                band_id: key.band_id,
                membership_id: key.membership_id,
                period_type_id: ki.period_type_id,
                position: ki.position,
                start_value_index: 0,
                value_count: ki.length,
                period_offset: ki.period_offset,
            };
            self.write_data_range_task_to_parquet(writer, &task, None)?;
        }

        Ok(())
    }

    fn populate_table_data_indexed_ranges(
        &self,
        con: &mut duckdb::Connection,
        plans: Vec<DataTableWritePlan>,
        worker_count: usize,
        staging_parent: &std::path::Path,
        deflate_index_interval_bytes: u64,
        progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        let tasks = self.build_data_range_write_tasks(&plans)?;
        if tasks.is_empty() {
            return Ok(());
        }

        let mut period_type_ids = std::collections::BTreeSet::new();
        for task in &tasks {
            period_type_ids.insert(task.period_type_id);
        }
        let compressed_indexes = self.build_compressed_period_data_indexes(
            staging_parent,
            deflate_index_interval_bytes,
            &period_type_ids,
            progress,
        )?;

        let worker_count = worker_count.min(tasks.len()).max(1);
        let total_ranges = tasks.len();
        let worker_tasks = Self::distribute_data_range_write_tasks(tasks, worker_count);
        Self::report_duckdb_progress(
            progress,
            &format!(
                "Writing indexed compressed BIN ranges with {} workers",
                worker_tasks.len()
            ),
        );

        let staging_dir = tempfile::Builder::new()
            .prefix("plexos2duckdb-range-parquet-")
            .tempdir_in(staging_parent)?;
        let (tx, rx) = std::sync::mpsc::channel::<DataRangeWriteWorkerEvent>();
        let staged_files = std::thread::scope(|scope| -> Result<Vec<StagedDataFiles>> {
            let mut handles = Vec::with_capacity(worker_tasks.len());
            for (worker_idx, worker_task_list) in worker_tasks.into_iter().enumerate() {
                let worker_dir = staging_dir
                    .path()
                    .join(format!("range_worker_{worker_idx}"));
                let worker_tx = tx.clone();
                let compressed_indexes = &compressed_indexes;

                handles.push(scope.spawn(move || -> Result<StagedDataFiles> {
                    std::fs::create_dir_all(&worker_dir)?;
                    let table_files = self.write_data_range_tasks_to_parquet_files(
                        worker_idx,
                        worker_task_list,
                        &worker_dir,
                        compressed_indexes,
                        &worker_tx,
                    )?;
                    Ok(StagedDataFiles::new(table_files))
                }));
            }
            drop(tx);

            let mut completed_ranges = 0usize;
            while completed_ranges < total_ranges {
                let event = match rx.recv() {
                    Ok(event) => event,
                    Err(_) => {
                        for (worker_idx, handle) in handles.into_iter().enumerate() {
                            let result = handle
                                .join()
                                .map_err(|_| eyre!("Range worker {} panicked", worker_idx + 1))?;
                            if let Err(err) = result {
                                return Err(eyre!("Range worker {} failed: {err}", worker_idx + 1));
                            }
                        }
                        return Err(eyre!(
                            "Range worker progress channel closed before all ranges completed ({}/{})",
                            completed_ranges,
                            total_ranges
                        ));
                    },
                };
                match event {
                    DataRangeWriteWorkerEvent::RangeStarted {
                        worker_id,
                        index,
                        total,
                        table_name,
                        key_id,
                        start_value,
                        values,
                    } => {
                        if let Some(report) = progress.as_mut() {
                            report(DuckdbProgress::Event(ProgressEvent::DataWorkerRangeStart {
                                worker_id,
                                index,
                                total,
                                table_name,
                                key_id,
                                start_value,
                                values,
                            }));
                        }
                    },
                    DataRangeWriteWorkerEvent::RangeCompleted {
                        worker_id,
                        index,
                        total,
                    } => {
                        if let Some(report) = progress.as_mut() {
                            report(DuckdbProgress::Event(ProgressEvent::DataWorkerRangeEnd {
                                worker_id,
                                index,
                                total,
                            }));
                        }
                        completed_ranges += 1;
                    },
                }
            }

            let mut staged_files = Vec::with_capacity(handles.len());
            Self::report_duckdb_progress(progress, "Finalizing indexed range parquet files");
            for (worker_idx, handle) in handles.into_iter().enumerate() {
                let result = handle
                    .join()
                    .map_err(|_| eyre!("Range worker {} panicked", worker_idx + 1))?;
                staged_files.push(
                    result.map_err(|err| eyre!("Range worker {} failed: {err}", worker_idx + 1))?,
                );
            }
            Ok(staged_files)
        })?;

        Self::report_duckdb_progress(progress, "Merging indexed range parquet files");
        self.merge_staged_data_files(con, &staged_files, progress)?;
        Ok(())
    }

    fn write_data_range_tasks_to_parquet_files(
        &self,
        worker_idx: usize,
        worker_task_list: Vec<DataRangeWriteTask>,
        worker_dir: &std::path::Path,
        compressed_indexes: &CompressedDeflateIndexes,
        worker_tx: &std::sync::mpsc::Sender<DataRangeWriteWorkerEvent>,
    ) -> Result<std::collections::BTreeMap<String, Vec<std::path::PathBuf>>> {
        let worker_total = worker_task_list.len();
        let mut table_files = std::collections::BTreeMap::<String, Vec<std::path::PathBuf>>::new();
        let mut current_table_name: Option<String> = None;
        let mut current_writer: Option<ArrowWriter<std::fs::File>> = None;
        let mut table_file_index = 0usize;

        for (task_idx, task) in worker_task_list.into_iter().enumerate() {
            if current_table_name.as_deref() != Some(task.table_name.as_str()) {
                if let Some(writer) = current_writer.take() {
                    writer.close()?;
                }

                table_file_index += 1;
                let parquet_path = worker_dir.join(format!("table_{table_file_index:05}.parquet"));
                current_writer = Some(Self::open_data_parquet_writer(&parquet_path)?);
                current_table_name = Some(task.table_name.clone());
                table_files
                    .entry(task.table_name.clone())
                    .or_default()
                    .push(parquet_path);
            }

            let worker_task_index = task_idx + 1;
            let _ = worker_tx.send(DataRangeWriteWorkerEvent::RangeStarted {
                worker_id: worker_idx,
                index: worker_task_index,
                total: worker_total,
                table_name: task.table_name.clone(),
                key_id: task.key_id,
                start_value: task.start_value_index,
                values: task.value_count,
            });

            let writer = current_writer
                .as_mut()
                .expect("range task writer is opened before writing");
            self.write_data_range_task_to_parquet(writer, &task, Some(compressed_indexes))?;

            let _ = worker_tx.send(DataRangeWriteWorkerEvent::completed(
                worker_idx,
                worker_task_index,
                worker_total,
            ));
        }

        if let Some(writer) = current_writer {
            writer.close()?;
        }

        Ok(table_files)
    }

    fn open_data_parquet_writer(path: &std::path::Path) -> Result<ArrowWriter<std::fs::File>> {
        let file = std::fs::File::create(path)?;
        let writer_properties = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .set_max_row_group_row_count(Some(DATA_APPEND_BATCH_VALUES as usize))
            .build();
        Ok(ArrowWriter::try_new(
            file,
            DATA_RECORD_BATCH_SCHEMA.clone(),
            Some(writer_properties),
        )?)
    }

    fn build_compressed_period_data_indexes(
        &self,
        _staging_parent: &std::path::Path,
        interval_bytes: u64,
        period_type_ids: &std::collections::BTreeSet<i64>,
        progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<CompressedDeflateIndexes> {
        let interval_bytes = interval_bytes.max(MIN_DEFLATE_INDEX_INTERVAL_BYTES);
        let compressed_entries: Vec<_> = self
            .period_data
            .iter()
            .filter(|(period_type_id, _period_data)| period_type_ids.contains(period_type_id))
            .filter_map(|(period_type_id, period_data)| {
                period_data
                    .compressed_zip_entry()
                    .map(|entry| (*period_type_id, entry))
            })
            .collect();

        let total_entries = compressed_entries.len();
        let mut indexes = DeflateIndexMap::with_capacity(compressed_entries.len());
        for (idx, (period_type_id, entry)) in compressed_entries.into_iter().enumerate() {
            Self::report_duckdb_progress(
                progress,
                &format!(
                    "Indexing {} ({}/{}, interval {} MiB)",
                    entry.name,
                    idx + 1,
                    total_entries,
                    interval_bytes / MIB_BYTES
                ),
            );
            let index = entry
                .build_deflate_index(interval_bytes)
                .map_err(|err| eyre!("Failed indexing compressed BIN '{}': {}", entry.name, err))?;
            let checkpoint_count = index.index.checkpoint_count();
            let window_bytes = index.index.window_bytes();
            Self::report_duckdb_progress(
                progress,
                &format!(
                    "Indexed {} into {} deflate checkpoints ({:.1} MiB)",
                    entry.name,
                    checkpoint_count,
                    window_bytes as f64 / MIB_BYTES as f64
                ),
            );
            indexes.insert(period_type_id, Arc::new(index));
        }

        Ok(CompressedDeflateIndexes {
            by_period_type_id: indexes,
        })
    }

    fn build_data_range_write_tasks(
        &self,
        plans: &[DataTableWritePlan],
    ) -> Result<Vec<DataRangeWriteTask>> {
        let mut tasks = Vec::new();
        for plan in plans {
            for key_id in plan.key_ids.iter().copied() {
                let ki = self.key_index(key_id)?;
                let key = self.key(key_id)?;
                if ki.position % 8 != 0 {
                    return Err(eyre!(
                        "BIN position misaligned for key_id {} (pos_bytes={})",
                        key_id,
                        ki.position
                    ));
                }

                let mut start_value_index = 0u64;
                while start_value_index < ki.length {
                    let value_count = (ki.length - start_value_index).min(DATA_RANGE_TASK_VALUES);
                    let byte_delta = start_value_index
                        .checked_mul(8)
                        .ok_or_else(|| eyre!("Byte offset overflow for key_id {}", key_id))?;
                    let position = ki
                        .position
                        .checked_add(byte_delta)
                        .ok_or_else(|| eyre!("Byte offset overflow for key_id {}", key_id))?;
                    tasks.push(DataRangeWriteTask {
                        table_name: plan.table_name.clone(),
                        key_id,
                        sample_id: key.sample_id,
                        band_id: key.band_id,
                        membership_id: key.membership_id,
                        period_type_id: ki.period_type_id,
                        position,
                        start_value_index,
                        value_count,
                        period_offset: ki.period_offset,
                    });
                    start_value_index = start_value_index
                        .checked_add(value_count)
                        .ok_or_else(|| eyre!("Value index overflow for key_id {}", key_id))?;
                }
            }
        }

        tasks.sort_by(|a, b| {
            b.estimated_values()
                .cmp(&a.estimated_values())
                .then_with(|| a.table_name.cmp(&b.table_name))
                .then_with(|| a.key_id.cmp(&b.key_id))
                .then_with(|| a.start_value_index.cmp(&b.start_value_index))
        });
        Ok(tasks)
    }

    fn distribute_data_range_write_tasks(
        tasks: Vec<DataRangeWriteTask>,
        worker_count: usize,
    ) -> Vec<Vec<DataRangeWriteTask>> {
        let mut worker_tasks = vec![Vec::<DataRangeWriteTask>::new(); worker_count];
        let mut worker_loads = vec![0u128; worker_count];

        for task in tasks {
            let (worker_idx, _) = worker_loads
                .iter()
                .enumerate()
                .min_by_key(|(_, load)| **load)
                .expect("worker_count must be non-zero");

            worker_loads[worker_idx] =
                worker_loads[worker_idx].saturating_add(task.estimated_values());
            worker_tasks[worker_idx].push(task);
        }

        for tasks in &mut worker_tasks {
            tasks.sort_by(|a, b| {
                a.table_name
                    .cmp(&b.table_name)
                    .then_with(|| a.key_id.cmp(&b.key_id))
                    .then_with(|| a.start_value_index.cmp(&b.start_value_index))
            });
        }
        worker_tasks.retain(|tasks| !tasks.is_empty());
        worker_tasks
    }

    fn write_data_range_task_to_parquet(
        &self,
        writer: &mut ArrowWriter<std::fs::File>,
        task: &DataRangeWriteTask,
        compressed_indexes: Option<&CompressedDeflateIndexes>,
    ) -> Result<()> {
        self.write_data_range_task_batches(task, compressed_indexes, |record_batch| {
            writer.write(&record_batch)?;
            Ok(())
        })
    }

    fn write_data_range_task_batches(
        &self,
        task: &DataRangeWriteTask,
        compressed_indexes: Option<&CompressedDeflateIndexes>,
        mut write_batch: impl FnMut(RecordBatch) -> Result<()>,
    ) -> Result<()> {
        let period_data = self
            .period_data
            .get(&task.period_type_id)
            .ok_or_else(|| eyre!("period type not found: {}", task.period_type_id))?;

        let mut chunk_buf = vec![0u8; (DATA_APPEND_BATCH_VALUES as usize) * 8];
        let mut value_offset = 0u64;

        match period_data {
            PeriodData::CompressedZipEntry(entry) => {
                let compressed_indexes = compressed_indexes.ok_or_else(|| {
                    eyre!(
                        "missing deflate seek indexes for compressed period type {}",
                        task.period_type_id
                    )
                })?;
                let index = compressed_indexes.get(task.period_type_id).ok_or_else(|| {
                    eyre!(
                        "missing deflate seek index for period type {}",
                        task.period_type_id
                    )
                })?;
                let mut decoder = entry.indexed_reader(index, task.position).map_err(|err| {
                    eyre!(
                        "Failed opening indexed compressed period data for key_id {} at byte offset {}: {}",
                        task.key_id,
                        task.position,
                        err
                    )
                })?;

                while value_offset < task.value_count {
                    let chunk_values =
                        (task.value_count - value_offset).min(DATA_APPEND_BATCH_VALUES);
                    let chunk_bytes = Self::data_chunk_byte_len(task.key_id, chunk_values)?;

                    decoder
                        .read_exact(&mut chunk_buf[..chunk_bytes])
                        .map_err(|err| {
                            eyre!(
                                "Failed reading indexed compressed period data for key_id {} at byte offset {}: {}",
                                task.key_id,
                                task.position + value_offset.saturating_mul(8),
                                err
                            )
                        })?;

                    Self::write_data_range_batch(
                        task,
                        value_offset,
                        &chunk_buf[..chunk_bytes],
                        &mut write_batch,
                    )?;
                    value_offset = value_offset
                        .checked_add(chunk_values)
                        .ok_or_else(|| eyre!("Value index overflow for key_id {}", task.key_id))?;
                }
            },
            _ => {
                while value_offset < task.value_count {
                    let chunk_values =
                        (task.value_count - value_offset).min(DATA_APPEND_BATCH_VALUES);
                    let chunk_bytes = Self::data_chunk_byte_len(task.key_id, chunk_values)?;
                    let offset_delta = value_offset
                        .checked_mul(8)
                        .ok_or_else(|| eyre!("Byte offset overflow for key_id {}", task.key_id))?;
                    let chunk_offset = task
                        .position
                        .checked_add(offset_delta)
                        .ok_or_else(|| eyre!("Byte offset overflow for key_id {}", task.key_id))?;

                    period_data
                        .read_exact_at(chunk_offset, &mut chunk_buf[..chunk_bytes])
                        .map_err(|err| {
                            eyre!(
                                "Failed reading period data for key_id {} at byte offset {}: {}",
                                task.key_id,
                                chunk_offset,
                                err
                            )
                        })?;

                    Self::write_data_range_batch(
                        task,
                        value_offset,
                        &chunk_buf[..chunk_bytes],
                        &mut write_batch,
                    )?;
                    value_offset = value_offset
                        .checked_add(chunk_values)
                        .ok_or_else(|| eyre!("Value index overflow for key_id {}", task.key_id))?;
                }
            },
        }

        Ok(())
    }

    fn write_data_range_batch(
        task: &DataRangeWriteTask,
        value_offset: u64,
        bytes: &[u8],
        write_batch: &mut impl FnMut(RecordBatch) -> Result<()>,
    ) -> Result<()> {
        let metadata = DataValueMetadata {
            key_id: task.key_id,
            sample_id: task.sample_id,
            band_id: task.band_id,
            membership_id: task.membership_id,
            period_offset: task.period_offset,
            start_value_index: task
                .start_value_index
                .checked_add(value_offset)
                .ok_or_else(|| eyre!("Value index overflow for key_id {}", task.key_id))?,
        };
        write_batch(metadata.record_batch(bytes)?)?;

        Ok(())
    }

    fn data_chunk_byte_len(key_id: i64, chunk_values: u64) -> Result<usize> {
        let chunk_bytes_u64 = chunk_values
            .checked_mul(8)
            .ok_or_else(|| eyre!("Chunk size overflow for key_id {}", key_id))?;
        usize::try_from(chunk_bytes_u64)
            .map_err(|_| eyre!("Chunk size exceeds usize for key_id {}", key_id))
    }

    fn merge_staged_data_files(
        &self,
        con: &mut duckdb::Connection,
        staged_files: &[StagedDataFiles],
        progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        let target_catalog = Self::current_catalog_name(con)?;
        let target_catalog_ident = Self::quote_ident(&target_catalog);
        let mut files_by_table =
            std::collections::BTreeMap::<String, Vec<std::path::PathBuf>>::new();
        for staged_file_set in staged_files {
            for (table_name, files) in &staged_file_set.table_files {
                files_by_table
                    .entry(table_name.clone())
                    .or_default()
                    .extend(files.iter().cloned());
            }
        }
        let total_merges = files_by_table.len();

        for (table_idx, (table_name, files)) in files_by_table.into_iter().enumerate() {
            let merge_index = table_idx + 1;
            if let Some(report) = progress.as_mut() {
                report(DuckdbProgress::Event(ProgressEvent::DataMergeTableStart {
                    index: merge_index,
                    total: total_merges,
                    table_name: table_name.clone(),
                }));
            }

            let table_ident = Self::quote_ident(&table_name);
            let parquet_paths =
                Self::sql_string_list(files.iter().map(|path| path.to_string_lossy().into_owned()));
            con.execute_batch(&format!(
                "INSERT INTO {target_catalog_ident}.data.{table_ident}
                 SELECT * FROM read_parquet({parquet_paths});"
            ))?;

            if let Some(report) = progress.as_mut() {
                report(DuckdbProgress::Event(ProgressEvent::DataMergeTableEnd {
                    index: merge_index,
                    total: total_merges,
                }));
            }
        }

        Ok(())
    }

    fn create_data_table(&self, con: &mut duckdb::Connection, table_name: &str) -> Result<()> {
        let table_ident = Self::quote_ident(table_name);
        con.execute_batch(&format!(
            r#"
            CREATE TABLE data.{table_ident} (
              key_id BIGINT,
              sample_id BIGINT,
              band_id BIGINT,
              membership_id BIGINT,
              block_id BIGINT,
              value DOUBLE
            )
        "#
        ))?;
        Ok(())
    }

    fn quote_ident(identifier: &str) -> String {
        format!("\"{}\"", identifier.replace('\"', "\"\""))
    }

    fn sql_string_literal(value: &str) -> String {
        value.replace('\'', "''")
    }

    fn sql_string_list(values: impl IntoIterator<Item = String>) -> String {
        let mut sql_values = Vec::new();
        for value in values {
            sql_values.push(format!("'{}'", Self::sql_string_literal(&value)));
        }
        format!("[{}]", sql_values.join(", "))
    }

    fn current_catalog_name(con: &duckdb::Connection) -> Result<String> {
        let mut stmt = con.prepare("SELECT current_catalog();")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Err(eyre!("Failed to resolve current DuckDB catalog name"))
        }
    }

    fn open_duckdb_write_connection(
        db_path: &std::path::Path,
    ) -> Result<(duckdb::Connection, tempfile::TempDir)> {
        let staging_parent = Self::duckdb_staging_parent(db_path);
        let staging_dir = tempfile::Builder::new()
            .prefix("plexos2duckdb-stage-")
            .tempdir_in(staging_parent)?;
        let staging_path = staging_dir.path().join("stage.duckdb");
        Ok((duckdb::Connection::open(staging_path)?, staging_dir))
    }

    fn duckdb_staging_parent(db_path: &std::path::Path) -> std::path::PathBuf {
        db_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf()
    }

    fn persist_duckdb_database(
        con: &mut duckdb::Connection,
        db_path: &std::path::Path,
    ) -> Result<()> {
        let target_catalog_ident = Self::quote_ident("persisted_database");
        let target_db_path = Self::sql_string_literal(db_path.to_string_lossy().as_ref());
        let source_catalog_ident = Self::quote_ident(&Self::current_catalog_name(con)?);

        con.execute_batch("PRAGMA force_checkpoint;")?;
        con.execute_batch("CHECKPOINT;")?;

        con.execute_batch(&format!(
            "
              ATTACH '{target_db_path}' AS {target_catalog_ident};
              COPY FROM DATABASE {source_catalog_ident} TO {target_catalog_ident};
              CHECKPOINT {target_catalog_ident};
              DETACH {target_catalog_ident};
            "
        ))?;
        Ok(())
    }

    pub(crate) fn read_exact_at(
        file: &std::fs::File,
        offset: u64,
        buf: &mut [u8],
    ) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt as _;
            return file.read_exact_at(buf, offset);
        }

        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt as _;

            let mut read_total = 0usize;
            while read_total < buf.len() {
                let n = file.seek_read(&mut buf[read_total..], offset + read_total as u64)?;
                if n == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "failed to fill whole buffer",
                    ));
                }
                read_total += n;
            }

            return Ok(());
        }

        #[cfg(not(any(unix, windows)))]
        {
            use std::io::{Read as _, Seek as _, SeekFrom};

            let mut clone = file.try_clone()?;
            clone.seek(SeekFrom::Start(offset))?;
            clone.read_exact(buf)
        }
    }

    fn populate_table_config(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
            CREATE TABLE raw.config (
                element VARCHAR,
                value VARCHAR
            );
        ",
        )?;

        let mut appender = con.appender_to_db("config", "raw")?;

        for (element, value) in &self.config {
            let value_str = &value.clone().unwrap_or_default();
            appender.append_row(&[element, value_str])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_memberships(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TYPE object_kind AS ENUM ('object', 'relation');
              CREATE TABLE raw.memberships (
                membership_id BIGINT PRIMARY KEY,
                collection_id BIGINT,
                collection VARCHAR,
                child_id BIGINT,
                child_name VARCHAR,
                child_category VARCHAR,
                child_category_class VARCHAR,
                parent_id BIGINT,
                parent_name VARCHAR,
                parent_category VARCHAR,
                parent_category_class VARCHAR,
                child_class_id BIGINT,
                child_class_name VARCHAR,
                parent_class_id BIGINT,
                parent_class_name VARCHAR,
                kind object_kind,
              );
              ",
        )?;

        let mut appender = con.appender_to_db("memberships", "raw")?;

        for membership in self.membership.values() {
            let child = self.object(membership.child_object_id)?;
            let parent = self.object(membership.parent_object_id)?;
            let child_category = self.category(child.category_id)?;
            let child_category_class = self.class(child_category.class_id)?;
            let child_class = self.class(membership.child_class_id)?;
            let parent_category = self.category(parent.category_id)?;
            let parent_category_class = self.class(parent_category.class_id)?;
            let parent_class = self.class(membership.parent_class_id)?;
            let collection_name = self.collection_name(membership.collection_id)?;
            let kind = if self.is_object(membership.collection_id)? {
                "object"
            } else {
                "relation"
            }
            .to_string();

            appender.append_row(duckdb::params![
                membership.membership_id,
                membership.collection_id,
                collection_name,
                child.object_id,
                child.name,
                child_category.name,
                child_category_class.name,
                parent.object_id,
                parent.name,
                parent_category.name,
                parent_category_class.name,
                child.class_id,
                child_class.name,
                parent.class_id,
                parent_class.name,
                kind,
            ])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_collections(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.collections (
                collection_id BIGINT PRIMARY KEY,
                parent_class_id BIGINT,
                child_class_id BIGINT,
                name VARCHAR,
                complement_name VARCHAR
              );
              ",
        )?;

        let mut appender = con.appender_to_db("collections", "raw")?;

        for collection in self.collection.values() {
            appender.append_row(duckdb::params![
                collection.collection_id,
                collection.parent_class_id,
                collection.child_class_id,
                collection.name,
                collection.complement_name
            ])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_classes(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.classes (
                class_id BIGINT PRIMARY KEY,
                name VARCHAR,
                class_group_id BIGINT
              );
              ",
        )?;

        let mut appender = con.appender_to_db("classes", "raw")?;

        for class in self.class.values() {
            appender.append_row(duckdb::params![
                class.class_id,
                class.name,
                class.class_group_id
            ])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_class_groups(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.class_groups (
                class_group_id BIGINT PRIMARY KEY,
                name VARCHAR
              );
              ",
        )?;

        let mut appender = con.appender_to_db("class_groups", "raw")?;

        for class_group in self.class_group.values() {
            appender.append_row(duckdb::params![
                class_group.class_group_id,
                class_group.name
            ])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_categories(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.categories (
                category_id BIGINT PRIMARY KEY,
                class_id BIGINT,
                rank BIGINT,
                name VARCHAR
              );
              ",
        )?;

        let mut appender = con.appender_to_db("categories", "raw")?;

        for category in self.category.values() {
            appender.append_row(duckdb::params![
                category.category_id,
                category.class_id,
                category.rank,
                category.name
            ])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_bands(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.bands (
                band_id BIGINT PRIMARY KEY,
              );
              ",
        )?;

        let mut appender = con.appender_to_db("bands", "raw")?;

        for band in self.band.values() {
            appender.append_row(duckdb::params![band])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_models(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.models (
                model_id BIGINT PRIMARY KEY,
                name VARCHAR
              );
              ",
        )?;

        let mut appender = con.appender_to_db("models", "raw")?;

        for model in self.model.values() {
            appender.append_row(duckdb::params![model.model_id, model.name])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_objects(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.objects (
                object_id BIGINT PRIMARY KEY,
                class_id BIGINT,
                name VARCHAR,
                category_id BIGINT,
                index BIGINT,
                is_show BOOLEAN,
              );
              ",
        )?;

        let mut appender = con.appender_to_db("objects", "raw")?;

        for object in self.object.values() {
            appender.append_row(duckdb::params![
                object.object_id,
                object.class_id,
                object.name,
                object.category_id,
                object.index,
                object.show,
            ])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_keys(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.keys (
                key_id BIGINT PRIMARY KEY,
                membership_id BIGINT,
                model_id BIGINT,
                phase_id BIGINT,
                property_id BIGINT,
                is_summary BOOLEAN,
                band_id BIGINT,
                sample_id BIGINT,
                timeslice_id BIGINT
              );
              ",
        )?;

        let mut appender = con.appender_to_db("keys", "raw")?;

        for key in self.key.values() {
            appender.append_row(duckdb::params![
                key.key_id,
                key.membership_id,
                key.model_id,
                key.phase_id,
                key.property_id,
                key.is_summary,
                key.band_id,
                key.sample_id,
                key.timeslice_id
            ])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_key_indexes(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.key_indexes (
                key_id BIGINT PRIMARY KEY,
                period_type_id BIGINT,
                position UBIGINT,
                length UBIGINT,
                period_offset BIGINT,
              );
              ",
        )?;

        let mut appender = con.appender_to_db("key_indexes", "raw")?;

        for key_index in self.key_index.values() {
            appender.append_row(duckdb::params![
                key_index.key_id,
                key_index.period_type_id,
                key_index.position,
                key_index.length,
                key_index.period_offset
            ])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_properties(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.properties (
                property_id BIGINT PRIMARY KEY,
                name VARCHAR,
                summary_name VARCHAR,
                enum_id BIGINT,
                unit_id BIGINT,
                summary_unit_id BIGINT,
                is_multi_band BOOLEAN,
                is_period BOOLEAN,
                is_summary BOOLEAN,
                collection_id BIGINT,
              );
              ",
        )?;
        let mut appender = con.appender_to_db("properties", "raw")?;
        for (_, property) in self.property.iter() {
            appender.append_row(duckdb::params![
                property.property_id,
                property.name,
                property.summary_name,
                property.enum_id,
                property.unit_id,
                property.summary_unit_id,
                property.is_multi_band,
                property.is_period,
                property.is_summary,
                property.collection_id,
            ])?;
        }
        appender.flush()?;

        Ok(())
    }

    fn populate_table_timeslices(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.timeslices (
                timeslice_id BIGINT PRIMARY KEY,
                timeslice_name VARCHAR,
              );
              ",
        )?;
        let mut appender = con.appender_to_db("timeslices", "raw")?;
        for (_, timeslice) in self.timeslice.iter() {
            appender.append_row(duckdb::params![timeslice.timeslice_id, timeslice.name])?;
        }
        appender.flush()?;

        Ok(())
    }

    fn populate_table_samples(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.samples (
                sample_id BIGINT PRIMARY KEY,
                sample_name VARCHAR,
                sample_phase_id BIGINT,
                sample_weight DOUBLE,
              );
              ",
        )?;
        let mut appender = con.appender_to_db("samples", "raw")?;
        for (_, sample) in self.sample.iter() {
            let sample_weight = self
                .sample_weight(sample.sample_id)
                .cloned()
                .unwrap_or_else(|_| SampleWeight {
                    sample_id: sample.sample_id,
                    phase_id: 0,
                    weight: 0.0,
                });

            appender.append_row(duckdb::params![
                sample.sample_id,
                sample.name,
                sample_weight.phase_id,
                sample_weight.weight
            ])?;
        }
        appender.flush()?;

        Ok(())
    }

    fn populate_table_units(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.units (
                unit_id BIGINT PRIMARY KEY,
                unit_name VARCHAR,
                lang_id BIGINT,
              );
              ",
        )?;
        let mut appender = con.appender_to_db("units", "raw")?;
        for (_, unit) in self.unit.iter() {
            appender.append_row(duckdb::params![unit.id, unit.value, unit.lang_id])?;
        }
        appender.flush()?;

        Ok(())
    }

    fn populate_table_memo_objects(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.memo_objects (
                object_id BIGINT,
                column_id BIGINT,
                value VARCHAR,
              );
              ",
        )?;
        let mut appender = con.appender_to_db("memo_objects", "raw")?;
        for memo in &self.memo_object {
            appender.append_row(duckdb::params![memo.object_id, memo.column_id, memo.value])?;
        }
        appender.flush()?;

        Ok(())
    }

    fn populate_table_custom_columns(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.custom_columns (
                column_id BIGINT PRIMARY KEY,
                name VARCHAR,
                position BIGINT,
                class_id BIGINT,
              );
              ",
        )?;
        let mut appender = con.appender_to_db("custom_columns", "raw")?;
        for (_, column) in self.custom_column.iter() {
            appender.append_row(duckdb::params![
                column.column_id,
                column.name,
                column.position,
                column.class_id
            ])?;
        }
        appender.flush()?;

        Ok(())
    }

    fn populate_table_attribute_data(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.attribute_data (
                object_id BIGINT,
                attribute_id BIGINT,
                value VARCHAR,
              );
              ",
        )?;
        let mut appender = con.appender_to_db("attribute_data", "raw")?;
        for (_, data) in self.attribute_data.iter() {
            if let Some(object_id) = data.object_id {
                appender.append_row(duckdb::params![object_id, data.attribute_id, data.value])?;
            }
        }
        appender.flush()?;

        Ok(())
    }

    fn populate_table_attributes(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.attributes (
                attribute_id BIGINT PRIMARY KEY,
                name VARCHAR,
                lang_id BIGINT,
                class_id BIGINT,
                description VARCHAR,
              );
              ",
        )?;

        let mut appender = con.appender_to_db("attributes", "raw")?;
        for (_, attribute) in self.attribute.iter() {
            appender.append_row(duckdb::params![
                attribute.attribute_id,
                attribute.name,
                attribute.lang_id,
                attribute.class_id,
                attribute.description
            ])?;
        }
        appender.flush()?;

        Ok(())
    }

    fn populate_table_metadata(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        con.execute_batch("CREATE TABLE main.plexos2duckdb (\n  key TEXT,\n  value TEXT\n);")?;

        let mut appender = con.appender("plexos2duckdb")?;
        appender.append_row(duckdb::params!["plexos2duckdb_version", utils::version()])?;
        appender.append_row(duckdb::params![
            "duckdb_file_created_at",
            chrono::Utc::now().to_string()
        ])?;
        appender.append_row(duckdb::params!["plexos_file", self.file.to_str()])?;
        appender.append_row(duckdb::params!["model_name", self.model_name])?;

        if let Some(log) = self.simulation_log.as_ref() {
            appender.append_row(duckdb::params!["simulation_log", log])?;
        }
        if let Some(run_stats) = self.run_stats.as_ref() {
            appender.append_row(duckdb::params!["run_stats", run_stats])?;
        }
        appender.flush()?;
        Ok(())
    }

    fn populate_table_timestamps_block(
        &self,
        con: &mut duckdb::Connection,
        _progress: &mut Option<&mut dyn FnMut(DuckdbProgress)>,
    ) -> Result<()> {
        for (name, values) in self.timestamp_block.iter() {
            con.execute_batch(&format!(
                "
                  CREATE TABLE raw.timestamp_block_{name} (
                    interval_id BIGINT,
                    datetime TIMESTAMP,
                  );
                ",
            ))?;

            let mut appender = con.appender_to_db(&format!("timestamp_block_{name}"), "raw")?;
            for (value, interval_id) in values.iter() {
                appender.append_row(duckdb::params![interval_id, value])?;
            }
            appender.flush()?;
        }

        Ok(())
    }

    fn create_report_views(
        &self,
        con: &mut duckdb::Connection,
        data_table_name_pattern: Option<&regex::Regex>,
    ) -> Result<()> {
        con.execute_batch("CREATE SCHEMA IF NOT EXISTS report;")?;

        for table_name in self.table_key_index_mapping.keys() {
            if !Self::data_table_name_matches(table_name, data_table_name_pattern) {
                continue;
            }

            let phase_name = table_name
                .split("__")
                .next()
                .ok_or_else(|| eyre!("Phase name not found"))?;
            let period_name = table_name
                .split("__")
                .nth(1)
                .ok_or_else(|| eyre!("Period name not found"))?;
            let property_name = table_name
                .split("__")
                .nth(3)
                .ok_or_else(|| eyre!("Property name not found"))?;
            con.execute_batch(&format!(
                "
                CREATE VIEW report.\"{table_name}\" AS SELECT
                  d.band_id AS band,
                  s.sample_name,
                  m.child_name AS name,
                  m.child_category AS category,
                  p.datetime AS timestamp,
                  p.interval_length AS interval_length,
                  d.value AS \"{property_name}\",
                  pr.unit AS unit,
                  FROM
                    data.\"{table_name}\" d
                    LEFT JOIN raw.samples s ON d.sample_id = s.sample_id
                    LEFT JOIN processed.memberships m ON d.membership_id = m.membership_id
                    LEFT JOIN processed.timestamp_block_{phase_name}__{period_name} p ON d.block_id = p.block_id
                    LEFT JOIN raw.keys k ON d.key_id = k.key_id
                    LEFT JOIN processed.properties pr ON k.property_id = pr.property_id AND k.is_summary = pr.is_summary
                  ORDER BY
                    d.band_id,
                    s.sample_id,
                    m.membership_id,
                    p.datetime
                  ;
                  ",
            ))?;
        }

        Ok(())
    }

    fn create_processed_views(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch("CREATE SCHEMA IF NOT EXISTS processed;")?;

        for (name, _) in self.timestamp_block.iter() {
            if name.contains("Interval") {
                con.execute_batch(&format!(
                    "
                    CREATE VIEW processed.timestamp_block_{name} AS
                      SELECT
                          interval_id AS block_id,
                          MIN(datetime) AS datetime,
                          COUNT(*) AS interval_length
                      FROM
                          raw.timestamp_block_{name}
                      GROUP BY
                          interval_id;
                    ",
                ))?;
            } else {
                con.execute_batch(&format!(
                    "
                    CREATE VIEW processed.timestamp_block_{name} AS
                      SELECT
                          ROW_NUMBER() OVER () AS block_id,
                          datetime,
                          1 AS interval_length,
                      FROM
                          raw.timestamp_block_{name};
                    ",
                ))?;
            }
        }

        con.execute_batch(
            "

        CREATE VIEW processed.classes AS
          SELECT
            c.class_id,
            c.name AS class,
            cg.name AS class_group
          FROM raw.classes c
          LEFT JOIN raw.class_groups cg
            ON c.class_group_id = cg.class_group_id;

        CREATE VIEW processed.objects AS
          SELECT
            o.object_id AS id,
            o.name AS name,
            cat.name AS category,
            c.class_group AS class_group,
            c.class AS class
          FROM raw.objects o
          JOIN processed.classes c
            ON o.class_id = c.class_id
          JOIN raw.categories cat
            ON o.category_id = cat.category_id;

        CREATE VIEW processed.properties AS
            SELECT
              p.property_id,
              false AS is_summary,
              c.name AS collection,
              p.name AS property,
              u.unit_name AS unit,
            FROM raw.properties p
            LEFT JOIN raw.collections c
              ON p.collection_id = c.collection_id
            LEFT JOIN raw.units u
              ON p.unit_id = u.unit_id
          UNION ALL
            SELECT
              p.property_id,
              true AS is_summary,
              c.name AS collection,
              p.summary_name AS property,
              u.unit_name AS unit,
            FROM raw.properties p
            LEFT JOIN raw.collections c
              ON p.collection_id = c.collection_id
            LEFT JOIN raw.units u
              ON p.summary_unit_id = u.unit_id;

        CREATE VIEW processed.memberships AS
          SELECT
            m.membership_id membership_id,
            m.parent_id parent_id,
            m.child_id child_id,
            c.name collection,
            p.name parent_name,
            p.class parent_class,
            p.class_group parent_group,
            p.category parent_category,
            ch.name child_name,
            ch.class child_class,
            ch.class_group child_group,
            ch.category child_category,
            m.kind kind,
          FROM raw.memberships m
          JOIN raw.collections c
            ON c.collection_id = m.collection_id
          JOIN processed.objects p
            ON p.id = m.parent_id
          JOIN processed.objects ch
            ON ch.id = m.child_id
          ",
        )?;

        Ok(())
    }

    fn collection_name(&self, collection_id: i64) -> Result<String> {
        let collection = self
            .collection
            .get(&collection_id)
            .ok_or(eyre!("Collection not found for {collection_id}"))?;
        let collection_name = collection.name.clone();

        let prefix = if let Some(name) = collection.complement_name.as_deref() {
            name
        } else {
            self.class
                .get(&collection.parent_class_id)
                .ok_or_else(|| eyre!("Parent class not found"))?
                .name
                .as_str()
        };

        Ok(format!("{prefix}_{collection_name}"))
    }

    fn object(&self, object_id: i64) -> Result<&Object> {
        self.object
            .get(&object_id)
            .ok_or_else(|| eyre!("Object not found for {object_id}"))
    }

    fn category(&self, category_id: i64) -> Result<&Category> {
        self.category
            .get(&category_id)
            .ok_or_else(|| eyre!("Category not found for {category_id}"))
    }

    fn attribute(&self, attribute_id: i64) -> Result<&Attribute> {
        self.attribute
            .get(&attribute_id)
            .ok_or_else(|| eyre!("Attribute not found for {attribute_id}"))
    }

    fn object_name(&self, object_id: i64) -> Result<String> {
        let object = self.object(object_id)?;
        let class = self.class(object.class_id)?;
        let category = self.category(object.category_id)?;

        Ok(format!("{}_{}_{}", class.name, category.name, object.name))
    }

    fn membership(&self, membership_id: i64) -> Result<&Membership> {
        self.membership
            .get(&membership_id)
            .ok_or_else(|| eyre!("Membership not found for {membership_id}"))
    }

    fn class(&self, class_id: i64) -> Result<&Class> {
        self.class
            .get(&class_id)
            .ok_or_else(|| eyre!("Class not found for {class_id}"))
    }

    fn collection(&self, collection_id: i64) -> Result<&Collection> {
        self.collection
            .get(&collection_id)
            .ok_or_else(|| eyre!("Collection not found for {collection_id}"))
    }

    fn property(&self, property_id: i64) -> Result<&Property> {
        self.property
            .get(&property_id)
            .ok_or_else(|| eyre!("Property not found for {property_id}"))
    }

    fn unit(&self, unit_id: i64) -> Result<&Unit> {
        self.unit
            .get(&unit_id)
            .ok_or_else(|| eyre!("Unit not found for {unit_id}"))
    }

    fn band(&self, band_id: i64) -> Result<&Band> {
        self.band
            .get(&band_id)
            .ok_or_else(|| eyre!("Band not found for {band_id}"))
    }

    fn sample(&self, sample_id: i64) -> Result<&Sample> {
        self.sample
            .get(&sample_id)
            .ok_or_else(|| eyre!("Sample not found for {sample_id}"))
    }

    fn sample_weight(&self, sample_id: i64) -> Result<&SampleWeight> {
        self.sample_weight
            .get(&sample_id)
            .ok_or_else(|| eyre!("Sample weight not found for {sample_id}"))
    }

    fn timeslice(&self, timeslice_id: i64) -> Result<&Timeslice> {
        self.timeslice
            .get(&timeslice_id)
            .ok_or_else(|| eyre!("Timeslice not found for {timeslice_id}"))
    }

    fn key_index(&self, key_id: i64) -> Result<&KeyIndex> {
        self.key_index
            .get(&key_id)
            .ok_or_else(|| eyre!("Key index not found for {key_id}"))
    }

    fn membership_name(&self, membership_id: i64) -> Result<String> {
        let membership = self.membership(membership_id)?;
        let collection_name = self.collection_name(membership.collection_id)?;
        let child_class = self.class(membership.child_class_id)?;
        let parent_class = self.class(membership.parent_class_id)?;
        let parent_object = self.object(membership.parent_object_id)?;
        let child_object = self.object(membership.child_object_id)?;
        Ok(format!(
            "{}_{}_{}_{}_{}",
            collection_name,
            parent_class.name,
            child_class.name,
            parent_object.name,
            child_object.name
        ))
    }

    fn interval(&self, interval_id: i64) -> Result<PeriodType> {
        self.period
            .get("interval")
            .ok_or_else(|| eyre!("Interval not found"))?
            .get(&interval_id)
            .cloned()
            .ok_or_else(|| eyre!("Interval not found for {interval_id}"))
    }

    fn day(&self, day_id: i64) -> Result<PeriodType> {
        self.period
            .get("day")
            .ok_or_else(|| eyre!("Day not found"))?
            .get(&day_id)
            .cloned()
            .ok_or_else(|| eyre!("Day not found for {day_id}"))
    }

    fn week(&self, week_id: i64) -> Result<PeriodType> {
        self.period
            .get("week")
            .ok_or_else(|| eyre!("Week not found"))?
            .get(&week_id)
            .cloned()
            .ok_or_else(|| eyre!("Week not found for {week_id}"))
    }

    fn month(&self, month_id: i64) -> Result<PeriodType> {
        self.period
            .get("month")
            .ok_or_else(|| eyre!("Month not found"))?
            .get(&month_id)
            .cloned()
            .ok_or_else(|| eyre!("Month not found for {month_id}"))
    }

    fn year(&self, fiscal_year_id: i64) -> Result<PeriodType> {
        self.period
            .get("year")
            .ok_or_else(|| eyre!("Year not found"))?
            .get(&fiscal_year_id)
            .cloned()
            .ok_or_else(|| eyre!("Year not found for {fiscal_year_id}"))
    }

    fn hour(&self, day_id: i64) -> Result<PeriodType> {
        self.period
            .get("hour")
            .ok_or_else(|| eyre!("Hour not found"))?
            .get(&day_id)
            .cloned()
            .ok_or_else(|| eyre!("Hour not found for {day_id}"))
    }

    fn quarter(&self, quarter_id: i64) -> Result<PeriodType> {
        self.period
            .get("quarter")
            .ok_or_else(|| eyre!("Quarter not found"))?
            .get(&quarter_id)
            .cloned()
            .ok_or_else(|| eyre!("Quarter not found for {quarter_id}"))
    }

    fn phase_name(&self, phase_id: i64) -> &str {
        match phase_id {
            1 => "LT",
            2 => "PASA",
            3 => "MT",
            4 => "ST",
            _ => "Unknown",
        }
    }

    fn period_name(&self, period_id: i64) -> &str {
        match period_id {
            0 => "Interval",
            1 => "Day",
            2 => "Week",
            3 => "Month",
            4 => "Year",
            6 => "Hour",
            7 => "Quarter",
            _ => "Unknown",
        }
    }

    fn is_object(&self, collection_id: i64) -> Result<bool> {
        let collection = self.collection(collection_id)?;
        let class = self.class(collection.parent_class_id)?;
        Ok(class.name == "System")
    }

    fn key(&self, key_id: i64) -> Result<Key> {
        self.key
            .get(&key_id)
            .cloned()
            .ok_or_else(|| eyre!("Key with {} not found", key_id))
    }

    pub fn print_summary(&self) {
        println!("Summary of PLEXOS solution dataset:");
        println!("  file: {}", self.file.display());
        println!("  models: {}", self.model.len());
        println!("  objects: {}", self.object.len());
        println!("  classes: {}", self.class.len());
        println!("  categories: {}", self.category.len());
        println!("  attributes: {}", self.attribute.len());
        println!("  properties: {}", self.property.len());
        println!("  units: {}", self.unit.len());
        println!("  bands: {}", self.band.len());
        println!("  collections: {}", self.collection.len());
        println!("  memberships: {}", self.membership.len());
        println!("  keys: {}", self.key.len());
        println!("  key indices: {}", self.key_index.len());
        println!("  attribute data: {}", self.attribute_data.len());
        println!("  custom columns: {}", self.custom_column.len());
        println!("  samples: {}", self.sample.len());
        println!("  timeslices: {}", self.timeslice.len());
        println!("  memo objects: {}", self.memo_object.len());
        println!("  custom columns: {}", self.custom_column.len());
        println!("  period data: {}", self.period_data.len());
        println!("  config: {}", self.config.len());
        println!("  attribute data: {}", self.attribute_data.len());
        println!("  period data: {}", self.period_data.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn build_small_database() -> Result<(tempfile::TempDir, std::path::PathBuf)> {
        let output_dir = tempfile::TempDir::new()?;
        let db_path = output_dir.path().join("result.duckdb");
        let (mut con, stage_dir) = SolutionDataset::open_duckdb_write_connection(&db_path)?;

        con.execute_batch(
            "
              CREATE TABLE numbers(value INTEGER);
              INSERT INTO numbers VALUES (1), (2), (3);
            ",
        )?;
        SolutionDataset::persist_duckdb_database(&mut con, &db_path)?;

        drop(con);
        drop(stage_dir);
        Ok((output_dir, db_path))
    }

    fn assert_small_database(db_path: &std::path::Path) -> Result<()> {
        let wal_path = std::path::PathBuf::from(format!("{}.wal", db_path.to_string_lossy()));
        assert!(
            db_path.exists(),
            "expected persisted database at {}",
            db_path.display()
        );
        assert!(
            !wal_path.exists(),
            "expected checkpointed database without WAL sidecar at {}",
            wal_path.display()
        );

        let con = duckdb::Connection::open(db_path)?;
        let mut stmt = con.prepare("SELECT COUNT(*), SUM(value) FROM numbers;")?;
        let mut rows = stmt.query([])?;
        let row = rows
            .next()?
            .ok_or_else(|| eyre!("expected numbers query to return a row"))?;
        let row_count: i64 = row.get(0)?;
        let total: i64 = row.get(1)?;
        assert_eq!(row_count, 3);
        assert_eq!(total, 6);
        Ok(())
    }

    fn file_period_table_specs() -> [(&'static str, [f64; 3]); 4] {
        [
            ("ST__Interval__FileFixture__MetricA", [1.0, 2.5, 3.75]),
            ("ST__Interval__FileFixture__MetricB", [10.0, 11.5, 12.75]),
            ("ST__Interval__FileFixture__MetricC", [20.0, 21.5, 22.75]),
            ("ST__Interval__FileFixture__MetricD", [30.0, 31.5, 32.75]),
        ]
    }

    fn write_file_period_data_database(
        threads: usize,
    ) -> Result<(tempfile::TempDir, std::path::PathBuf)> {
        let output_dir = tempfile::TempDir::new()?;
        let bin_path = output_dir.path().join("t_data_0.BIN");
        let mut bin_file = std::fs::File::create(&bin_path)?;

        let mut key = indexmap::IndexMap::new();
        let mut key_index = indexmap::IndexMap::new();
        let mut table_key_index_mapping = std::collections::HashMap::new();
        let mut byte_position = 0u64;

        for (table_idx, (table_name, values)) in file_period_table_specs().into_iter().enumerate() {
            for value in values {
                bin_file.write_all(&value.to_le_bytes())?;
            }

            let key_id = 100 + i64::try_from(table_idx)?;
            let sample_id = 200 + i64::try_from(table_idx)?;
            let band_id = 300 + i64::try_from(table_idx)?;
            let membership_id = 400 + i64::try_from(table_idx)?;
            let period_offset = i64::try_from(table_idx)? * 10;
            key.insert(
                key_id,
                Key {
                    key_id,
                    phase_id: 4,
                    is_summary: false,
                    band_id,
                    membership_id,
                    model_id: 1,
                    property_id: 500 + i64::try_from(table_idx)?,
                    sample_id,
                    timeslice_id: 0,
                },
            );
            key_index.insert(
                key_id,
                KeyIndex {
                    key_id,
                    period_type_id: 0,
                    length: u64::try_from(values.len())?,
                    position: byte_position,
                    period_offset,
                },
            );
            table_key_index_mapping.insert(table_name.to_string(), vec![key_id]);
            let bytes_written = u64::try_from(values.len())?
                .checked_mul(8)
                .ok_or_else(|| eyre!("test BIN byte length overflow"))?;
            byte_position = byte_position
                .checked_add(bytes_written)
                .ok_or_else(|| eyre!("test BIN byte position overflow"))?;
        }
        drop(bin_file);

        let mut period_data = indexmap::IndexMap::new();
        period_data.insert(0, std::fs::File::open(&bin_path)?);

        let base_time = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")?
            .with_timezone(&chrono::Utc);
        let mut timestamp_block = std::collections::HashMap::new();
        let mut timestamps = Vec::new();
        for block_id in 1_i64..=40 {
            timestamps.push((base_time + chrono::Duration::hours(block_id - 1), block_id));
        }
        timestamp_block.insert("ST__Interval".to_string(), timestamps);

        let dataset = SolutionDataset {
            file: output_dir.path().join("source.xml"),
            model_name: "file-period-data".to_string(),
            key,
            key_index,
            period_data: period_data
                .into_iter()
                .map(|(period_type_id, file)| (period_type_id, PeriodData::File(file)))
                .collect(),
            timestamp_block,
            table_key_index_mapping,
            ..Default::default()
        };

        let db_path = output_dir
            .path()
            .join(format!("file_period_threads_{threads}.duckdb"));
        dataset
            .to_duckdb(&db_path)
            .with_data_write_threads(threads)
            .run()?;

        Ok((output_dir, db_path))
    }

    fn assert_file_period_data(db_path: &std::path::Path) -> Result<()> {
        let con = duckdb::Connection::open(db_path)?;
        for (table_idx, (table_name, expected_values)) in
            file_period_table_specs().into_iter().enumerate()
        {
            let key_id = 100 + i64::try_from(table_idx)?;
            let sample_id = 200 + i64::try_from(table_idx)?;
            let band_id = 300 + i64::try_from(table_idx)?;
            let membership_id = 400 + i64::try_from(table_idx)?;
            let period_offset = i64::try_from(table_idx)? * 10;
            let table_ident = SolutionDataset::quote_ident(table_name);
            let mut stmt = con.prepare(&format!(
                "SELECT key_id, sample_id, band_id, membership_id, block_id, value
                 FROM data.{table_ident}
                 ORDER BY block_id;"
            ))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, f64>(5)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            assert_eq!(rows.len(), expected_values.len());
            for (value_idx, row) in rows.into_iter().enumerate() {
                let expected_block_id = period_offset + i64::try_from(value_idx)? + 1;
                assert_eq!(row.0, key_id);
                assert_eq!(row.1, sample_id);
                assert_eq!(row.2, band_id);
                assert_eq!(row.3, membership_id);
                assert_eq!(row.4, expected_block_id);
                assert_eq!(row.5, expected_values[value_idx]);
            }
        }
        Ok(())
    }

    #[test]
    fn persist_database_uses_stage_copy_and_checkpoint() -> Result<()> {
        let (_output_dir, db_path) = build_small_database()?;
        assert_small_database(&db_path)
    }

    #[test]
    fn file_period_data_uses_parquet_staging_with_one_thread() -> Result<()> {
        let (_output_dir, db_path) = write_file_period_data_database(1)?;
        assert_file_period_data(&db_path)
    }

    #[test]
    fn file_period_data_uses_parquet_staging_with_four_threads() -> Result<()> {
        let (_output_dir, db_path) = write_file_period_data_database(4)?;
        assert_file_period_data(&db_path)
    }

    #[test]
    fn read_zip_archive_indexes_deflated_bin_entries_without_buffering() -> Result<()> {
        let temp_dir = tempfile::TempDir::new()?;
        let zip_path = temp_dir.path().join("Model_Base_Solution.zip");

        let zip_file = std::fs::File::create(&zip_path)?;
        let mut zip_writer = zip::ZipWriter::new(zip_file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip_writer.start_file("Model Base Solution.xml", options)?;
        zip_writer.write_all(b"<root />")?;

        zip_writer.start_file("t_data_0.BIN", options)?;
        let expected = [1.25_f64, 2.5_f64];
        for value in expected {
            zip_writer.write_all(&value.to_le_bytes())?;
        }
        zip_writer.finish()?;

        let (_xml_content, period_data) =
            SolutionDataset::read_zip_archive(&zip_path, "", &mut None)?;
        let data = period_data
            .get(&0)
            .ok_or_else(|| eyre!("expected period data for digit 0"))?;

        let entry = match data {
            PeriodData::CompressedZipEntry(entry) => entry,
            _ => return Err(eyre!("expected compressed ZIP period data metadata")),
        };
        let index = entry.build_deflate_index(8)?;
        let mut actual = [0u8; 16];
        entry.read_exact_at_with_index(&index, 0, &mut actual)?;

        let first = f64::from_le_bytes(actual[0..8].try_into().expect("slice length matches"));
        let second = f64::from_le_bytes(actual[8..16].try_into().expect("slice length matches"));

        assert_eq!(first, 1.25);
        assert_eq!(second, 2.5);
        Ok(())
    }

    #[test]
    fn deflate_index_reads_random_offsets_from_zip_member() -> Result<()> {
        let temp_dir = tempfile::TempDir::new()?;
        let zip_path = temp_dir.path().join("Model_Base_Solution.zip");
        let value_count = 300_000usize;
        let mut expected = Vec::with_capacity(value_count * 8);

        let zip_file = std::fs::File::create(&zip_path)?;
        let mut zip_writer = zip::ZipWriter::new(zip_file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip_writer.start_file("Model Base Solution.xml", options)?;
        zip_writer.write_all(b"<root />")?;

        zip_writer.start_file("t_data_0.BIN", options)?;
        let mut state = 0x1234_5678_9abc_def0u64;
        for _ in 0..value_count {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let value = f64::from_bits(state);
            let bytes = value.to_le_bytes();
            expected.extend_from_slice(&bytes);
            zip_writer.write_all(&bytes)?;
        }
        zip_writer.finish()?;

        let (_xml_content, period_data) =
            SolutionDataset::read_zip_archive(&zip_path, "", &mut None)?;
        let data = period_data
            .get(&0)
            .ok_or_else(|| eyre!("expected period data for digit 0"))?;
        let entry = match data {
            PeriodData::CompressedZipEntry(entry) => entry,
            _ => return Err(eyre!("expected compressed ZIP period data metadata")),
        };
        let index = entry.build_deflate_index(MIB_BYTES)?;
        assert!(
            index.index.checkpoint_count() > 0,
            "expected at least one deflate checkpoint"
        );

        for offset in [0usize, 8 * 123, 8 * 4097 + 3, expected.len() - 19] {
            let mut actual = vec![0u8; 19];
            entry.read_exact_at_with_index(&index, offset as u64, &mut actual)?;
            assert_eq!(actual, expected[offset..offset + 19]);
        }

        Ok(())
    }
}

/// Helper function to get text from a child element, returns any type T that implements FromStr
fn get_child<T: std::str::FromStr>(node: &Node, tag_name: &str) -> Result<T>
where
    T::Err: std::fmt::Debug,
{
    node.children()
        .find(|n| n.has_tag_name(tag_name))
        .and_then(|n| n.text())
        .map(|s| s.to_string())
        .ok_or_else(|| eyre!("Missing {} element: {:?}", tag_name, node))?
        .parse::<T>()
        .map_err(|_| eyre!("Invalid value for {}: {:?}", tag_name, node))
}

fn parse_datetime_to_utc(input: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    // Try parsing with timezone first
    if let Ok(dt_with_tz) = chrono::DateTime::parse_from_rfc3339(input) {
        Ok(dt_with_tz.with_timezone(&chrono::Utc))
    } else {
        let naive = chrono::NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S")?;
        Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
            naive,
            chrono::Utc,
        ))
    }
}
