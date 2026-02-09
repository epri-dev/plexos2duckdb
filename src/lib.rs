#![allow(dead_code)]
#![doc = include_str!("../README.md")]

use std::io::Read as _;

use color_eyre::{Result, eyre::eyre};
use roxmltree::{Document, Node};

pub mod utils;

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
    length: usize,                // in 8-byte (64-bit float) increments
    position: usize,              // bytes from binary file start
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
        if self.is_summary { self.summary_name.clone() } else { self.name.clone() }
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
    period_data: indexmap::IndexMap<i64, memmap2::Mmap>,
    simulation_log: Option<String>,
    run_stats: Option<String>,
    // calculated fields
    timestamp_block: std::collections::HashMap<String, Vec<(chrono::DateTime<chrono::Utc>, i64)>>,
    table_key_index_mapping: std::collections::HashMap<String, Vec<i64>>,
    table_units_mapping: std::collections::HashMap<String, (String, i64)>,
}

impl SolutionDataset {
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
        self.category.values().filter(|c| c.class_id == class_id).collect()
    }

    pub fn with_model_name(mut self, model_name: String) -> Self {
        if model_name.is_empty() {
            return self;
        }
        self.model_name = model_name;
        self
    }

    pub fn with_period_data(mut self, period_data: indexmap::IndexMap<i64, memmap2::Mmap>) -> Self {
        self.period_data = period_data;
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

        RE.captures(name).and_then(|cap| cap.get(1)).and_then(|m| m.as_str().parse::<i64>().ok())
    }

    pub fn with_zip_file<P: AsRef<std::path::Path>>(self, path: P) -> Result<Self> {
        let file = std::fs::File::open(&path)?;

        // Get the zip file's stem (base name without extension)
        let zip_stem =
            path.as_ref().file_stem().ok_or_else(|| eyre!("Invalid zip file name"))?.to_string_lossy().to_string();

        let mut archive = zip::ZipArchive::new(file)?;

        // Find the preferred XML file in the archive
        let mut xml_content = String::new();
        let mut preferred_xml_index = None;
        let mut model_name_xml_index = None;
        let mut first_xml_index = None;

        // Use the model name from self.model_name if available, otherwise infer from file name
        let model_name = if !self.model_name.is_empty() {
            Some(self.model_name.to_lowercase())
        } else {
            // Infer model name from file name
            let file_name = path.as_ref().file_name().and_then(|s| s.to_str()).unwrap_or_default();
            Some(file_name.trim_start_matches("Model ").trim_end_matches(" Solution.zip").to_lowercase())
        };

        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let file_name = file.name().to_string();
            let file_name_lower = file_name.to_lowercase();
            if file_name_lower.ends_with(".xml") {
                if let Some(stem) = std::path::Path::new(&file_name).file_stem().map(|s| s.to_string_lossy()) {
                    if stem == zip_stem {
                        preferred_xml_index = Some(i);
                        break;
                    }
                    if model_name.as_ref().map_or(false, |mn| stem.to_lowercase().contains(mn.as_str())) {
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

        let mut file = archive.by_index(xml_index_to_use)?;
        file.read_to_string(&mut xml_content)?;
        drop(file);

        // Prepare a temporary directory to extract BIN files
        let temp_dir = tempfile::TempDir::new()?;
        let mut period_data = indexmap::IndexMap::new();

        let archive_len = archive.len();
        for i in 0..archive_len {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();

            // Use the validation function to check filename and extract digit
            if let Some(digit) = Self::is_valid_bin_filename(&name) {
                // Extract file to temp dir with a safe filename
                let safe_filename = format!("t_data_{}.BIN", digit);
                let temp_file_path = temp_dir.path().join(&safe_filename);

                let mut out_file = std::fs::File::create(&temp_file_path)?;
                std::io::copy(&mut file, &mut out_file)?;
                // Ensure data is flushed to disk
                drop(out_file);

                // Memory-map the extracted file
                let mmap_file = std::fs::File::open(&temp_file_path)?;
                let mmap = unsafe { memmap2::Mmap::map(&mmap_file)? };

                period_data.insert(digit, mmap);
            }
        }

        Ok(self.with_file(path).with_xml_string(&xml_content)?.with_period_data(period_data))
    }

    pub fn with_xml_file<P: AsRef<std::path::Path>>(self, path: P) -> Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        self.with_xml_string(&content)
    }

    pub fn with_xml_string(mut self, xml: &str) -> Result<Self> {
        let doc = Document::parse(xml)?;

        let root = doc.root_element();
        self.parse_attribute_data(&root)?;
        self.parse_attribute(&root)?;
        self.parse_property(&root)?;
        self.parse_band(&root)?;
        self.parse_category(&root)?;
        self.parse_class_group(&root)?;
        self.parse_classes(&root)?;
        self.parse_collection(&root)?;
        self.parse_config(&root)?;
        self.parse_key_index(&root)?;
        self.parse_key(&root)?;
        self.parse_membership(&root)?;
        self.parse_models(&root)?;
        self.parse_object(&root)?;
        self.parse_period0(&root)?;
        self.parse_period1(&root)?;
        self.parse_period2(&root)?;
        self.parse_period3(&root)?;
        self.parse_period4(&root)?;
        self.parse_period6(&root)?;
        self.parse_period7(&root)?;
        self.parse_phase1(&root)?;
        self.parse_phase2(&root)?;
        self.parse_phase3(&root)?;
        self.parse_phase4(&root)?;
        self.parse_sample(&root)?;
        self.parse_sample_weight(&root)?;
        self.parse_timeslice(&root)?;
        self.parse_unit(&root)?;
        self.parse_memo_object(&root)?;
        self.parse_custom_column(&root)?;

        self.update_property_band_id()?;
        self.update_timestamp_block()?;
        self.update_collection_membership_count()?;
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

            let object = Object { class_id, name, category_id, index, object_id, show, guid };
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

            let attribute = Attribute { attribute_id, class_id, enum_id, name, description, lang_id, input_mask };
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

            let unit = Unit { id: unit_id, value, lang_id };

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

            let category = Category { category_id, class_id, rank, name };
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

            let class = Class { class_id, name, class_group_id, lang_id, state };
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

            let class_group = ClassGroup { class_group_id, name, lang_id, state };
            self.class_group.insert(class_group.class_group_id, class_group);
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

            let key_index = KeyIndex { key_id, period_type_id, position, length, period_offset };
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
            let datetime =
                chrono::DateTime::parse_from_str(&format!("{datetime} +0000"), "%d/%m/%Y %H:%M:%S %z")?.into();
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
        self.period.entry("interval".to_string()).or_default().sort_keys();
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
            let period1 = Period1 { day_id, date, week_id, month_id, fiscal_year_id, quarter_id };

            self.period.entry("day".to_string()).or_default().insert(period1.day_id, PeriodType::Day(period1));
        }
        self.period.entry("day".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_period2(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_2")) {
            let week_id = get_child(&period_node, "week_id")?;
            let week_ending: String = get_child(&period_node, "week_ending")?;
            let week_ending = parse_datetime_to_utc(&week_ending)?;
            let period2 = Period2 { week_id, week_ending };
            self.period.entry("week".to_string()).or_default().insert(period2.week_id, PeriodType::Week(period2));
        }
        self.period.entry("week".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_period3(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_3")) {
            let month_id = get_child(&period_node, "month_id")?;
            let month_beginning: String = get_child(&period_node, "month_beginning")?;
            let month_beginning = parse_datetime_to_utc(&month_beginning)?;
            let period3 = Period3 { month_id, month_beginning };
            self.period.entry("month".to_string()).or_default().insert(period3.month_id, PeriodType::Month(period3));
        }
        self.period.entry("month".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_period4(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_4")) {
            let fiscal_year_id = get_child(&period_node, "fiscal_year_id")?;
            let year_ending: String = get_child(&period_node, "year_ending")?;
            let year_ending = parse_datetime_to_utc(&year_ending)?;
            let period4 = Period4 { fiscal_year_id, year_ending };
            self.period
                .entry("year".to_string())
                .or_default()
                .insert(period4.fiscal_year_id, PeriodType::Year(period4));
        }
        self.period.entry("year".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_period6(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_6")) {
            let hour_id = get_child(&period_node, "hour_id")?;
            let datetime: String = get_child(&period_node, "datetime")?;
            let datetime = parse_datetime_to_utc(&datetime)?;
            let period6 = Period6 { hour_id, datetime };
            self.period.entry("hour".to_string()).or_default().insert(period6.hour_id, PeriodType::Hour(period6));
        }
        self.period.entry("hour".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_period7(&mut self, node: &Node) -> Result<()> {
        for period_node in node.children().filter(|n| n.has_tag_name("t_period_7")) {
            let quarter_id = get_child(&period_node, "quarter_id")?;
            let quarter_beginning: String = get_child(&period_node, "quarter_beginning")?;
            let quarter_beginning = parse_datetime_to_utc(&quarter_beginning)?;
            let period7 = Period7 { quarter_id, quarter_beginning };
            self.period
                .entry("quarter".to_string())
                .or_default()
                .insert(period7.quarter_id, PeriodType::Quarter(period7));
        }
        self.period.entry("quarter".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_phase1(&mut self, node: &Node) -> Result<()> {
        for phase_node in node.children().filter(|n| n.has_tag_name("t_phase_1")) {
            let interval_id = get_child(&phase_node, "interval_id")?;
            let period_id = get_child(&phase_node, "period_id")?;

            let phase1 = Phase1 { interval_id, period_id };
            self.phase.entry("LT".to_string()).or_default().insert(phase1.interval_id, PhaseType::LT(phase1));
        }
        self.phase.entry("LT".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_phase2(&mut self, node: &Node) -> Result<()> {
        for phase_node in node.children().filter(|n| n.has_tag_name("t_phase_2")) {
            let interval_id = get_child(&phase_node, "interval_id")?;
            let period_id = get_child(&phase_node, "period_id")?;

            let phase2 = Phase2 { interval_id, period_id };
            self.phase.entry("PASA".to_string()).or_default().insert(phase2.interval_id, PhaseType::PASA(phase2));
        }
        self.phase.entry("PASA".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_phase3(&mut self, node: &Node) -> Result<()> {
        for phase_node in node.children().filter(|n| n.has_tag_name("t_phase_3")) {
            let interval_id = get_child(&phase_node, "interval_id")?;
            let period_id = get_child(&phase_node, "period_id")?;

            let phase3 = Phase3 { interval_id, period_id };
            self.phase.entry("MT".to_string()).or_default().insert(phase3.interval_id, PhaseType::MT(phase3));
        }
        self.phase.entry("MT".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_phase4(&mut self, node: &Node) -> Result<()> {
        for phase_node in node.children().filter(|n| n.has_tag_name("t_phase_4")) {
            let interval_id = get_child(&phase_node, "interval_id")?;
            let period_id = get_child(&phase_node, "period_id")?;

            let phase4 = Phase4 { interval_id, period_id };
            self.phase.entry("ST".to_string()).or_default().insert(phase4.interval_id, PhaseType::ST(phase4));
        }
        self.phase.entry("ST".to_string()).or_default().sort_keys();
        Ok(())
    }

    fn parse_sample(&mut self, node: &Node) -> Result<()> {
        for sample_node in node.children().filter(|n| n.has_tag_name("t_sample")) {
            let id = get_child(&sample_node, "sample_id")?;
            let name = get_child(&sample_node, "sample_name").ok();

            let sample = Sample { sample_id: id, name };
            self.sample.insert(sample.sample_id, sample);
        }
        self.sample.sort_keys();
        Ok(())
    }

    fn parse_sample_weight(&mut self, node: &Node) -> Result<()> {
        for sample_weight_node in node.children().filter(|n| n.has_tag_name("t_sample_weight")) {
            let sample_id = get_child(&sample_weight_node, "sample_id")?;
            let phase_id = get_child(&sample_weight_node, "phase_id")?;
            let weight = get_child(&sample_weight_node, "value")?;

            let sample_weight = SampleWeight { sample_id, phase_id, weight };
            self.sample_weight.insert(sample_weight.sample_id, sample_weight);
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
        for attribute_node in node.children().filter(|n| n.has_tag_name("t_attribute_data")) {
            let object_id = get_child(&attribute_node, "object_id").ok();
            let attribute_id = get_child(&attribute_node, "attribute_id")?;
            let value = get_child(&attribute_node, "value")?;

            let attribute_data = AttributeData { object_id, attribute_id, value };
            self.attribute_data.insert(attribute_data.attribute_id, attribute_data);
        }
        self.attribute_data.sort_keys();
        Ok(())
    }

    fn parse_memo_object(&mut self, node: &Node) -> Result<()> {
        for memo_node in node.children().filter(|n| n.has_tag_name("t_memo_object")) {
            let value = get_child(&memo_node, "value")?;
            let column_id = get_child(&memo_node, "column_id")?;
            let object_id = get_child(&memo_node, "object_id")?;

            let memo_object = MemoObject { value, column_id, object_id };
            self.memo_object.push(memo_object);
        }

        Ok(())
    }

    fn parse_custom_column(&mut self, node: &Node) -> Result<()> {
        for custom_column_node in node.children().filter(|n| n.has_tag_name("t_custom_column")) {
            let column_id = get_child(&custom_column_node, "column_id")?;
            let name = get_child(&custom_column_node, "name")?;
            let position = get_child(&custom_column_node, "position")?;
            let class_id = get_child(&custom_column_node, "class_id")?;
            let custom_column = CustomColumn { column_id, name, position, class_id };
            self.custom_column.insert(custom_column.column_id, custom_column);
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
                        self.timestamp_block.entry(key).or_default().push((datetime, period_id));
                    }
                }
            }
        }

        Ok(())
    }

    fn update_collection_membership_count(&mut self) -> Result<()> {
        // count how many memberships per collection
        let mut membership_counts: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();

        for membership in self.membership.values() {
            *membership_counts.entry(membership.collection_id).or_insert(0) += 1;
        }

        // assign 0-based index per membership and update collection's n_members
        let mut collection_indices: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();

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
        let mut key_indexes_mapping: std::collections::HashMap<String, Vec<i64>> = Default::default();
        let mut units_mapping: std::collections::HashMap<String, (String, i64)> = Default::default();

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
            let property_name = if key.is_summary { property.summary_name() } else { property.property_name() };
            let unit_id = if property.is_summary { property.summary_unit_id } else { property.unit_id };
            let unit = self.unit(unit_id)?;
            let unit_name = unit.value.clone();
            let period_offset = ki.period_offset;

            let table_name = format!("{phase_name}__{period_name}__{collection_name}__{property_name}")
                .replace(" ", "_")
                .replace("-", "_");

            key_indexes_mapping.entry(table_name.clone()).or_default().push(key_id);
            units_mapping.insert(table_name, (unit_name, period_offset));
        }
        self.table_key_index_mapping = key_indexes_mapping;
        self.table_units_mapping = units_mapping;
        Ok(())
    }

    pub fn to_duckdb<P: AsRef<std::path::Path>>(&self, db_path: P) -> Result<()> {
        let db_path = db_path.as_ref();
        let mut con = duckdb::Connection::open_in_memory()?;

        con.execute_batch("SET preserve_insertion_order = false;")?;

        con.execute_batch("CREATE SCHEMA IF NOT EXISTS raw;")?;

        self.populate_table_metadata(&mut con)?;

        self.populate_table_config(&mut con)?;
        self.populate_table_memberships(&mut con)?;
        self.populate_table_collections(&mut con)?;
        self.populate_table_classes(&mut con)?;
        self.populate_table_class_groups(&mut con)?;
        self.populate_table_categories(&mut con)?;
        self.populate_table_bands(&mut con)?;
        self.populate_table_models(&mut con)?;
        self.populate_table_objects(&mut con)?;
        self.populate_table_keys(&mut con)?;
        self.populate_table_key_indexes(&mut con)?;
        self.populate_table_properties(&mut con)?;
        self.populate_table_timeslices(&mut con)?;
        self.populate_table_samples(&mut con)?;
        self.populate_table_units(&mut con)?;
        self.populate_table_memo_objects(&mut con)?;
        self.populate_table_custom_columns(&mut con)?;
        self.populate_table_attribute_data(&mut con)?;
        self.populate_table_attributes(&mut con)?;
        self.populate_table_timestamps_block(&mut con)?;

        self.populate_table_data(&mut con)?;

        self.create_processed_views(&mut con)?;

        self.create_report_views(&mut con)?;

        con.execute_batch(&format!(
            "
              ATTACH '{}' as my_database;
              COPY FROM DATABASE memory TO my_database;
              DETACH my_database;
            ",
            db_path.to_str().unwrap_or_default()
        ))?;

        Ok(())
    }

    fn populate_table_config(&self, con: &mut duckdb::Connection) -> Result<()> {
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

    fn populate_table_memberships(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TYPE object_kind AS ENUM ('object', 'relation');
              CREATE TABLE raw.memberships (
                membership_id INTEGER PRIMARY KEY,
                collection_id INTEGER,
                collection VARCHAR,
                child_id INTEGER,
                child_name VARCHAR,
                child_category VARCHAR,
                child_category_class VARCHAR,
                parent_id INTEGER,
                parent_name VARCHAR,
                parent_category VARCHAR,
                parent_category_class VARCHAR,
                child_class_id INTEGER,
                child_class_name VARCHAR,
                parent_class_id INTEGER,
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
            let kind = if self.is_object(membership.collection_id)? { "object" } else { "relation" }.to_string();

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

    fn populate_table_collections(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.collections (
                collection_id INTEGER PRIMARY KEY,
                parent_class_id INTEGER,
                child_class_id INTEGER,
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

    fn populate_table_classes(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.classes (
                class_id INTEGER PRIMARY KEY,
                name VARCHAR,
                class_group_id INTEGER
              );
              ",
        )?;

        let mut appender = con.appender_to_db("classes", "raw")?;

        for class in self.class.values() {
            appender.append_row(duckdb::params![class.class_id, class.name, class.class_group_id])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_class_groups(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.class_groups (
                class_group_id INTEGER PRIMARY KEY,
                name VARCHAR
              );
              ",
        )?;

        let mut appender = con.appender_to_db("class_groups", "raw")?;

        for class_group in self.class_group.values() {
            appender.append_row(duckdb::params![class_group.class_group_id, class_group.name])?;
        }

        appender.flush()?;

        Ok(())
    }

    fn populate_table_categories(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.categories (
                category_id INTEGER PRIMARY KEY,
                class_id INTEGER,
                rank INTEGER,
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

    fn populate_table_bands(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.bands (
                band_id INTEGER PRIMARY KEY,
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

    fn populate_table_models(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.models (
                model_id INTEGER PRIMARY KEY,
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

    fn populate_table_objects(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.objects (
                object_id INTEGER PRIMARY KEY,
                class_id INTEGER,
                name VARCHAR,
                category_id INTEGER,
                index INTEGER,
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

    fn populate_table_keys(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.keys (
                key_id BIGINT PRIMARY KEY,
                membership_id INTEGER,
                model_id INTEGER,
                phase_id INTEGER,
                property_id INTEGER,
                is_summary BOOLEAN,
                band_id INTEGER,
                sample_id INTEGER,
                timeslice_id INTEGER
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

    fn populate_table_key_indexes(&self, con: &mut duckdb::Connection) -> Result<()> {
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

    fn populate_table_properties(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.properties (
                property_id INTEGER PRIMARY KEY,
                name VARCHAR,
                summary_name VARCHAR,
                enum_id INTEGER,
                unit_id INTEGER,
                summary_unit_id INTEGER,
                is_multi_band BOOLEAN,
                is_period BOOLEAN,
                is_summary BOOLEAN,
                collection_id INTEGER,
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

    fn populate_table_timeslices(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.timeslices (
                timeslice_id INTEGER PRIMARY KEY,
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

    fn populate_table_samples(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.samples (
                sample_id INTEGER PRIMARY KEY,
                sample_name VARCHAR,
                sample_phase_id INTEGER,
                sample_weight DOUBLE,
              );
              ",
        )?;
        let mut appender = con.appender_to_db("samples", "raw")?;
        for (_, sample) in self.sample.iter() {
            let sample_weight = self
                .sample_weight(sample.sample_id)
                .cloned()
                .unwrap_or_else(|_| SampleWeight { sample_id: sample.sample_id, phase_id: 0, weight: 0.0 });

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

    fn populate_table_units(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.units (
                unit_id INTEGER PRIMARY KEY,
                unit_name VARCHAR,
                lang_id INTEGER,
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

    fn populate_table_memo_objects(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.memo_objects (
                object_id INTEGER,
                column_id INTEGER,
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

    fn populate_table_custom_columns(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.custom_columns (
                column_id INTEGER PRIMARY KEY,
                name VARCHAR,
                position INTEGER,
                class_id INTEGER,
              );
              ",
        )?;
        let mut appender = con.appender_to_db("custom_columns", "raw")?;
        for (_, column) in self.custom_column.iter() {
            appender.append_row(duckdb::params![column.column_id, column.name, column.position, column.class_id])?;
        }
        appender.flush()?;

        Ok(())
    }

    fn populate_table_attribute_data(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.attribute_data (
                object_id INTEGER,
                attribute_id INTEGER,
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

    fn populate_table_attributes(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch(
            "
              CREATE TABLE raw.attributes (
                attribute_id INTEGER PRIMARY KEY,
                name VARCHAR,
                lang_id INTEGER,
                class_id INTEGER,
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

    fn populate_table_metadata(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch("CREATE TABLE main.plexos2duckdb (\n  key TEXT,\n  value TEXT\n);")?;

        let mut appender = con.appender("plexos2duckdb")?;
        appender.append_row(duckdb::params!["plexos2duckdb_version", utils::version()])?;
        appender.append_row(duckdb::params!["duckdb_file_created_at", chrono::Utc::now().to_string()])?;
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

    fn populate_table_timestamps_block(&self, con: &mut duckdb::Connection) -> Result<()> {
        for (name, values) in self.timestamp_block.iter() {
            con.execute_batch(&format!(
                "
                  CREATE TABLE raw.timestamp_block_{name} (
                    interval_id INTEGER,
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

    fn populate_table_data(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch("CREATE SCHEMA IF NOT EXISTS data;")?;

        for (table_name, key_ids) in self.table_key_index_mapping.clone() {
            con.execute_batch(&format!(
                "
                CREATE TABLE data.\"{table_name}\" (
                  key_id BIGINT,
                  sample_id INTEGER,
                  band_id INTEGER,
                  membership_id INTEGER,
                  block_id INTEGER,
                  value DOUBLE,
                )
              ",
            ))?;

            // TODO: check whether period_offset is the starting period of the timeseries data

            let mut appender = con.appender_to_db(&table_name, "data")?;

            for key_id in key_ids {
                let ki = self.key_index(key_id)?;
                let key = self.key(key_id)?;

                let band_id = key.band_id;
                let sample_id = key.sample_id;
                let membership_id = key.membership_id;

                let start_idx = ki.position;
                let end_idx = ki.position + 8 * ki.length;
                let raw_data =
                    self.period_data.get(&ki.period_type_id).ok_or_else(|| eyre!("period type not found"))?;
                let raw_data = &raw_data[start_idx..end_idx];
                let data = raw_data.chunks_exact(8).map(TryInto::try_into).map(Result::unwrap).map(f64::from_le_bytes);
                let period_offset = ki.period_offset as usize;

                // dimension 1 is the enumerated time
                for (block_id, value) in data.enumerate() {
                    appender.append_row(duckdb::params![
                        key_id,
                        sample_id,
                        band_id,
                        membership_id,
                        block_id + period_offset + 1,
                        value
                    ])?;
                }
            }
            appender.flush()?;
        }

        Ok(())
    }

    fn create_report_views(&self, con: &mut duckdb::Connection) -> Result<()> {
        con.execute_batch("CREATE SCHEMA IF NOT EXISTS report;")?;

        for table_name in self.table_key_index_mapping.keys() {
            let phase_name = table_name.split("__").next().ok_or_else(|| eyre!("Phase name not found"))?;
            let period_name = table_name.split("__").nth(1).ok_or_else(|| eyre!("Period name not found"))?;
            let property_name = table_name.split("__").nth(3).ok_or_else(|| eyre!("Property name not found"))?;
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
        let collection =
            self.collection.get(&collection_id).ok_or(eyre!("Collection not found for {collection_id}"))?;
        let collection_name = collection.name.clone();

        let prefix = if let Some(name) = collection.complement_name.as_deref() {
            name
        } else {
            self.class.get(&collection.parent_class_id).ok_or_else(|| eyre!("Parent class not found"))?.name.as_str()
        };

        Ok(format!("{prefix}_{collection_name}"))
    }

    fn object(&self, object_id: i64) -> Result<&Object> {
        self.object.get(&object_id).ok_or_else(|| eyre!("Object not found for {object_id}"))
    }

    fn category(&self, category_id: i64) -> Result<&Category> {
        self.category.get(&category_id).ok_or_else(|| eyre!("Category not found for {category_id}"))
    }

    fn attribute(&self, attribute_id: i64) -> Result<&Attribute> {
        self.attribute.get(&attribute_id).ok_or_else(|| eyre!("Attribute not found for {attribute_id}"))
    }

    fn object_name(&self, object_id: i64) -> Result<String> {
        let object = self.object(object_id)?;
        let class = self.class(object.class_id)?;
        let category = self.category(object.category_id)?;

        Ok(format!("{}_{}_{}", class.name, category.name, object.name))
    }

    fn membership(&self, membership_id: i64) -> Result<&Membership> {
        self.membership.get(&membership_id).ok_or_else(|| eyre!("Membership not found for {membership_id}"))
    }

    fn class(&self, class_id: i64) -> Result<&Class> {
        self.class.get(&class_id).ok_or_else(|| eyre!("Class not found for {class_id}"))
    }

    fn collection(&self, collection_id: i64) -> Result<&Collection> {
        self.collection.get(&collection_id).ok_or_else(|| eyre!("Collection not found for {collection_id}"))
    }

    fn property(&self, property_id: i64) -> Result<&Property> {
        self.property.get(&property_id).ok_or_else(|| eyre!("Property not found for {property_id}"))
    }

    fn unit(&self, unit_id: i64) -> Result<&Unit> {
        self.unit.get(&unit_id).ok_or_else(|| eyre!("Unit not found for {unit_id}"))
    }

    fn band(&self, band_id: i64) -> Result<&Band> {
        self.band.get(&band_id).ok_or_else(|| eyre!("Band not found for {band_id}"))
    }

    fn sample(&self, sample_id: i64) -> Result<&Sample> {
        self.sample.get(&sample_id).ok_or_else(|| eyre!("Sample not found for {sample_id}"))
    }

    fn sample_weight(&self, sample_id: i64) -> Result<&SampleWeight> {
        self.sample_weight.get(&sample_id).ok_or_else(|| eyre!("Sample weight not found for {sample_id}"))
    }

    fn timeslice(&self, timeslice_id: i64) -> Result<&Timeslice> {
        self.timeslice.get(&timeslice_id).ok_or_else(|| eyre!("Timeslice not found for {timeslice_id}"))
    }

    fn key_index(&self, key_id: i64) -> Result<&KeyIndex> {
        self.key_index.get(&key_id).ok_or_else(|| eyre!("Key index not found for {key_id}"))
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
            collection_name, parent_class.name, child_class.name, parent_object.name, child_object.name
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
        self.key.get(&key_id).cloned().ok_or_else(|| eyre!("Key with {} not found", key_id))
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
        Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc))
    }
}
