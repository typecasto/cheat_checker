#![allow(unused, dead_code)]

use encoding_rs::Encoding;
use indicatif::ProgressBar;
use owo_colors::{style, OwoColorize};
use std::collections::BinaryHeap;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::{process, thread};
use strsim::{normalized_damerau_levenshtein, normalized_levenshtein};

#[derive(Debug, Clone, bpaf::Bpaf)]
#[bpaf(options, version)]
struct CheatCheck {

    /// Lower bound for cheat detection.
    ///
    /// Between 0 and 1, where 1 means identical files.
    #[bpaf(short, long, argument("SENSITIVITY"))]
    sensitivity: f32,

    /// Number of calculations to run in parallel.
    /// 
    /// The default is 0, meaning autodetect.
    #[bpaf(short, long, argument("N"), fallback(0))]
    jobs: usize,

    /// Logs all comparisons to this file.
    #[bpaf(short, long("log"), argument("FILE"))]
    logfile: Option<PathBuf>,

    /// Use Damerau-Levenshtein distance instead of Levenshtein distance.
    ///
    /// About 20x slower.
    #[bpaf(short('D'), long("damerau"), switch)]
    damerau_mode: bool,

    /// Program used to format code before checking
    ///
    /// Before comparing two files, we'll run them both through this program.
    /// Improves detection, since changing the format won't affect the results
    /// anymore.
    /// 
    /// TODO
    #[bpaf(short, long, argument("PROGRAM"))]
    formatter: Option<String>,

    /// Ignored file for cheat detection.
    ///
    /// If a file matches this file exactly, it will not be cheat checked.
    /// This is intended to avoid the situation where several students didn't
    /// do the assignment, and thus have exactly the same file turned in.
    /// 
    /// TODO
    #[bpaf(short, long, argument("FILE"))]
    template: Option<PathBuf>,

    /// Files or globs of files to compare.
    #[bpaf(positional("FILE"))]
    files: Vec<String>,
}

/// Takes a list of paths and turns them into paths matching files
fn filter_paths(globs: &Vec<String>) -> Vec<PathBuf> {
    let mut all_files: Vec<PathBuf> = Vec::new();
    for glob in globs {
        let paths = glob::glob(glob); // i enjoy this line
        match paths {
            Ok(paths) => {
                let count = all_files.len();
                all_files.extend(paths.filter_map(Result::ok));
                if count == all_files.len() {
                    log::warn!("\"{}\" didn't match any files.", &glob);
                }
            }
            Err(err) => {
                log::warn!(
                    "\"{}\" is not a valid pattern, and will be ignored. ({})",
                    &glob,
                    &err.msg
                );
            }
        }
    }
    globs
        .iter()
        .map(std::fs::canonicalize)
        .filter_map(Result::ok)
        .collect()
}

/// Loads a file to a string, handling non-utf-8 encoding
fn load_file(path: &PathBuf, program: &CheatCheck) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes);
    let encoding = chardet::detect(&bytes).0;
    let encoding = Encoding::for_label(encoding.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    //TODO preprocessing
    Ok(encoding.decode(&bytes).0.to_string())
}

// compares two paths and gives a float result
fn compare(x: &str, y: &str, opts: &CheatCheck) -> anyhow::Result<f64> {
    if opts.damerau_mode {
        return Ok(normalized_damerau_levenshtein(x, y));
    } else {
        return Ok(normalized_levenshtein(x, y));
    }
}

fn main() {
    // --- Process arguments and file list
    pretty_env_logger::init();
    let mut opts = cheat_check().run();
    if opts.jobs == 0 {
        opts.jobs = thread::available_parallelism()
            .unwrap_or(NonZeroUsize::new(1).unwrap()).into();
    }
    let opts = opts;
    let mut paths = filter_paths(&opts.files);
    // make sure we have enough files
    if paths.len() <= 1 {
        log::error!("Got {} files to compare, need at least 2.", paths.len());
        return;
    } else {
        log::info!("Got {} files to compare.", paths.len())
    }
    
    // --- Compare files
    let bar = ProgressBar::new((paths.len() * paths.len() / 2) as u64);
    // TODO load all files into memory beforehand
    for x in &paths {
        let fx = load_file(x, &opts).unwrap();
        for y in &paths {
            // skip this comparison if we've already compared the two in opposite direction
            // or if it's the same file twice
            // NB: if the same file is given more than once on the command line, this will cause the
            // progress bar to desync. do we care?
            if x >= y {
                continue;
            }
            let fy = load_file(y, &opts).unwrap();
            let similarity = compare(&fx, &fy, &opts).unwrap();
            // TODO multithread this for 4x increase ez
            // TODO log all to a file
            if &similarity >= &opts.sensitivity.into() {
                bar.println(format!(
                    "{}\n{}\n\t{:0<1.3}",
                    x.as_os_str().to_string_lossy(),
                    y.as_os_str().to_string_lossy(),
                    similarity.black().on_red()
                ));
            }
            bar.inc(1);
        }
    }
}

#[cfg(test)]
#[test]
fn check_opts() {
    cheat_check().check_invariants(true);
}
