use std::time::{Duration, Instant};

use clap::Parser;
use color_eyre::{
    Result,
    eyre::{ContextCompat, eyre},
};
use console::Term;
use ctrlc;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use owo_colors::OwoColorize;
use plexos2duckdb;

#[derive(Parser)]
#[command(author, version = plexos2duckdb::utils::version(), about, long_about = None)]
struct Args {
    /// Path to the PLEXOS solution file or folder (either XML or ZIP containing XML, or solution folder)
    #[arg(short, long)]
    input: std::path::PathBuf,
    /// Path to the output DuckDB file (leave empty to use the same name as input)
    #[arg(short, long)]
    output: Option<std::path::PathBuf>,
    /// Print a summary of the dataset
    #[arg(long, default_value_t = false)]
    print_summary: bool,
    /// Disable progress bar output
    #[arg(long, default_value_t = false)]
    no_progress_bar: bool,
    /// Stage in memory then copy to disk (faster, but uses more RAM)
    #[arg(long, default_value_t = false)]
    in_memory: bool,
    /// Number of threads to use when writing time series data tables
    #[arg(long)]
    data_write_threads: Option<std::num::NonZeroUsize>,
}

fn resolve_input_path(input: &std::path::Path) -> Result<std::path::PathBuf> {
    let path = if input.is_file() {
        let ext = input.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());

        match ext.as_deref() {
            Some("zip") | Some("xml") => input.to_path_buf(),
            _ => return Err(eyre!("Input file must have .zip or .xml extension")),
        }
    } else if input.is_dir() {
        let mut zip_files = std::fs::read_dir(input)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("zip")))
            .collect::<Vec<_>>();

        if zip_files.len() == 1 {
            zip_files.remove(0)
        } else if zip_files.is_empty() {
            return Err(eyre!("No .zip files found in directory"));
        } else {
            return Err(eyre!("Multiple .zip files found in directory"));
        }
    } else {
        return Err(eyre!("Path is neither a file nor a directory: {}", input.display()));
    };
    if !path.exists() {
        return Err(eyre!("File or directory does not exist: {}", input.display()));
    }
    Ok(path)
}

fn resolve_output_path(input: &std::path::PathBuf, output: Option<std::path::PathBuf>) -> Result<std::path::PathBuf> {
    let output_path = if let Some(output_path) = output { output_path } else { input.with_extension("duckdb") };
    let output_path =
        if output_path.extension().is_none() { output_path.with_extension("duckdb") } else { output_path };
    if output_path.exists() {
        std::fs::remove_file(&output_path)?;
    }
    Ok(output_path)
}

fn run(args: Args) -> Result<()> {
    let input_path = resolve_input_path(&args.input)?;
    let input_dir = input_path.parent().ok_or_else(|| eyre!("Input path has no parent directory"))?;
    let output_path = resolve_output_path(&input_path, args.output)?;

    let mut mp = None;
    let mut pb = None;
    let mut data_tables_pb = None;
    let mut worker_tables_pb: std::collections::BTreeMap<usize, ProgressBar> = std::collections::BTreeMap::new();
    let mut current_table = None;
    let mut last_msg = String::new();
    let mut start_time = None;
    let mut last_mark = None;
    let mut total_line = None;
    let mut term = None;
    if !args.no_progress_bar {
        let term_handle = Term::stderr();
        let _ = term_handle.hide_cursor();
        let multi = MultiProgress::new();
        multi.set_draw_target(ProgressDrawTarget::term(term_handle.clone(), 120));
        let spinner = multi.add(ProgressBar::new_spinner());
        spinner.set_style(ProgressStyle::with_template("{spinner:.green} {elapsed_precise:.dim} {msg}").unwrap());
        spinner.enable_steady_tick(Duration::from_millis(120));
        start_time = Some(Instant::now());
        last_mark = start_time;
        pb = Some(spinner);
        mp = Some(multi);
        term = Some(term_handle);
    }
    if let Some(term) = term.clone() {
        let _ = ctrlc::set_handler(move || {
            let _ = term.show_cursor();
            eprintln!();
            std::process::exit(130);
        });
    }
    struct CursorGuard(Option<Term>);
    impl Drop for CursorGuard {
        fn drop(&mut self) {
            if let Some(term) = self.0.as_ref() {
                let _ = term.show_cursor();
            }
        }
    }
    let _cursor_guard = CursorGuard(term.clone());
    let mut report = |msg: &str| {
        if let Some(spinner) = pb.as_ref() {
            if msg != last_msg {
                let now = Instant::now();
                if !last_msg.is_empty() {
                    let delta = last_mark.map(|s| now.duration_since(s)).unwrap_or_default();
                    let line = format!("[+{:>6.2}s]", delta.as_secs_f64()).dimmed().to_string();
                    let msg = last_msg.cyan().to_string();
                    spinner.println(format!("{line} {msg}"));
                }
                spinner.set_message(msg.to_string());
                last_msg.clear();
                last_msg.push_str(msg);
                last_mark = Some(now);
                if let Some(start) = start_time {
                    let total = now.duration_since(start);
                    total_line = Some(format!("Total time: {:.2}s", total.as_secs_f64()));
                }
            }
        }
    };

    let mut report_data = |event: plexos2duckdb::ProgressEvent| {
        if args.no_progress_bar {
            return;
        }
        match event {
            plexos2duckdb::ProgressEvent::DataTableStart { index, total, table_name, keys } => {
                if keys == 0 {
                    return;
                }
                current_table = Some(table_name);
                if data_tables_pb.is_none() {
                    if let Some(multi) = mp.as_ref() {
                        let bar = multi.add(ProgressBar::new(total as u64));
                        bar.set_style(
                            ProgressStyle::with_template(
                                "{bar:40.cyan/blue} {pos:>2}/{len:2} {elapsed_precise:.dim} {msg:.cyan}",
                            )
                            .unwrap(),
                        );
                        data_tables_pb = Some(bar);
                    }
                }
                if let Some(bar) = data_tables_pb.as_ref() {
                    bar.set_length(total as u64);
                    bar.set_position(index as u64);
                    let table = current_table.as_deref().unwrap_or("data");
                    bar.set_message(format!("{table} ({keys} keys)"));
                }
            },
            plexos2duckdb::ProgressEvent::DataTableEnd => {
                current_table = None;
            },
            plexos2duckdb::ProgressEvent::DataWorkerTableStart { worker_id, index, total, table_name, keys } => {
                if !worker_tables_pb.contains_key(&worker_id) {
                    if let Some(multi) = mp.as_ref() {
                        let bar = multi.add(ProgressBar::new(total as u64));
                        bar.set_style(
                            ProgressStyle::with_template(
                                "{prefix:>9.bold} {bar:28.green/blue} {pos:>2}/{len:2} {elapsed_precise:.dim} {msg:.green}",
                            )
                            .unwrap(),
                        );
                        bar.set_prefix(format!("thread-{}", worker_id + 1));
                        worker_tables_pb.insert(worker_id, bar);
                    }
                }
                if let Some(bar) = worker_tables_pb.get(&worker_id) {
                    bar.set_length(total as u64);
                    bar.set_position(index.saturating_sub(1) as u64);
                    bar.set_message(format!("{table_name} ({keys} keys)"));
                }
            },
            plexos2duckdb::ProgressEvent::DataWorkerTableEnd { worker_id, index, total } => {
                if let Some(bar) = worker_tables_pb.get(&worker_id) {
                    bar.set_length(total as u64);
                    bar.set_position(index as u64);
                    if index == total {
                        bar.set_message("done");
                    }
                }
            },
        }
    };

    // Extract model name from the file name
    let file_name =
        input_path.file_name().context("File name must exist")?.to_str().context("File name must be valid UTF-8")?;
    let model_name = file_name
        .trim_start_matches("Model ")
        .trim_end_matches(" Solution") // if input_path is a folder
        .trim_end_matches(" Solution.zip") // if input_path is a zip file
        .trim_end_matches(" Solution.xml"); // if input path is a xml file

    let dataset = {
        let actual_input_path = if input_path.is_dir() {
            let mut zip_files = std::fs::read_dir(&input_path)?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("zip")))
                .collect::<Vec<_>>();
            if zip_files.len() == 1 {
                zip_files.remove(0)
            } else if zip_files.is_empty() {
                return Err(eyre!("No .zip files found in directory"));
            } else {
                return Err(eyre!("Multiple .zip files found in directory"));
            }
        } else {
            input_path.clone()
        };
        if actual_input_path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("zip")) {
            let mut ds = plexos2duckdb::SolutionDataset::default()
                .with_model_name(model_name.to_string())
                .with_zip_file_with_progress(&actual_input_path, &mut report)?;
            // Look for a log file with the correct model name pattern
            let log_path = actual_input_path
                .parent()
                .ok_or_else(|| eyre!("Could not determine parent directory for input file"))?
                .join(format!("Model ( {} ) Log.txt", model_name));
            if log_path.exists() {
                report("Reading simulation log");
                let log = std::fs::read_to_string(&log_path)?;
                ds = ds.with_simulation_log(log);
            }
            ds
        } else if actual_input_path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("xml")) {
            let mut ds = plexos2duckdb::SolutionDataset::default()
                .with_model_name(model_name.to_string())
                .with_xml_file_with_progress(&actual_input_path, &mut report)?;
            let log_path = input_dir.join(format!("Model ( {} ) Log.txt", model_name));
            if log_path.exists() {
                report("Reading simulation log");
                let log = std::fs::read_to_string(&log_path)?;
                ds = ds.with_simulation_log(log);
            }
            ds
        } else {
            return Err(eyre!("Input file must have .zip or .xml extension"));
        }
    };

    let run_stats = input_dir.join(std::path::Path::new("runstats.json"));
    let dataset = if let Ok(run_stats) = std::fs::read_to_string(&run_stats) {
        report("Reading run stats");
        dataset.with_run_stats(run_stats)
    } else {
        dataset
    };

    if args.print_summary {
        if let Some(spinner) = pb.as_ref() {
            if !last_msg.is_empty() {
                let now = Instant::now();
                let delta = last_mark.map(|s| now.duration_since(s)).unwrap_or_default();
                let line = format!("[+{:>6.2}s]", delta.as_secs_f64()).dimmed().to_string();
                let msg = last_msg.cyan().to_string();
                spinner.println(format!("{line} {msg}"));
            }
            spinner.finish_and_clear();
        }
        if let Some(term) = term.as_ref() {
            let _ = term.show_cursor();
        }
        if let Some(bar) = data_tables_pb.as_ref() {
            bar.finish_and_clear();
        }
        for bar in worker_tables_pb.values() {
            bar.finish_and_clear();
        }
        if let Some(line) = total_line.as_ref() {
            eprintln!("{}", line.green());
        }
        dataset.print_summary();
        return Ok(());
    }
    report("Creating DuckDB database");
    let mode =
        if args.in_memory { plexos2duckdb::DbWriteMode::InMemoryThenCopy } else { plexos2duckdb::DbWriteMode::Direct };
    let mut builder = dataset.to_duckdb(&output_path).with_mode(mode);
    if let Some(threads) = args.data_write_threads {
        builder = builder.with_data_write_threads(threads.get());
    }
    let builder =
        if !args.no_progress_bar { builder.with_progress(&mut report).with_events(&mut report_data) } else { builder };
    builder.run()?;
    if let Some(spinner) = pb.as_ref() {
        if !last_msg.is_empty() {
            let now = Instant::now();
            let delta = last_mark.map(|s| now.duration_since(s)).unwrap_or_default();
            let line = format!("[+{:>6.2}s]", delta.as_secs_f64()).dimmed().to_string();
            let msg = last_msg.cyan().to_string();
            spinner.println(format!("{line} {msg}"));
        }
        spinner.finish_and_clear();
    }
    if let Some(term) = term.as_ref() {
        let _ = term.show_cursor();
    }
    if let Some(bar) = data_tables_pb.as_ref() {
        bar.finish_and_clear();
    }
    for bar in worker_tables_pb.values() {
        bar.finish_and_clear();
    }
    if let Some(line) = total_line.as_ref() {
        eprintln!("{}", line.green());
    }
    println!("{} {}", "DuckDB database created at:".green(), output_path.display().to_string().blue());
    Ok(())
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    run(args)
}
