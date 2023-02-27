// #![allow(unused, dead_code)]

use encoding_rs::Encoding;
use indicatif::ProgressBar;
use log::LevelFilter::{Debug, Info};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, bpaf::Bpaf)]
#[bpaf(options, version)]
struct CliArgs {
    /// Lower bound for cheat detection.
    ///
    /// Between 0 and 1, where 1 means identical files.
    #[bpaf(short, long, argument("SENSITIVITY"))]
    sensitivity: f64,

    /// Number of calculations to run in parallel.
    ///
    /// The default is 0, meaning autodetect.
    #[bpaf(short, long, argument("N"), fallback(0))]
    jobs: usize,

    /// Show additional debugging information.
    #[bpaf(short, long, switch)]
    verbose: bool,

    /// Logs all comparisons to this file.
    #[bpaf(short, long("log"), argument("FILE"), hide)]
    _logfile: Option<PathBuf>,

    /// Program used to format code before checking
    ///
    /// Before comparing two files, we'll run them both through this program.
    /// Improves detection, since changing the format won't affect the results
    /// anymore.
    ///
    /// TODO
    #[bpaf(short, long, argument("PROGRAM"), hide)]
    _formatter: Option<String>,

    /// Ignored file for cheat detection.
    ///
    /// If a file matches this file exactly, it will not be cheat checked.
    /// This is intended to avoid the situation where several students didn't
    /// do the assignment, and thus have exactly the same file turned in.
    ///
    /// TODO
    #[bpaf(short, long, argument("FILE"), hide)]
    _template: Option<PathBuf>,

    /// Files or globs of files to compare.
    #[bpaf(positional("FILE"))]
    files: Vec<PathBuf>,
}

/// Takes a list of paths and turns them into paths matching files
fn filter_paths(globs: &Vec<PathBuf>) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    for pattern in globs {
        let pattern = pattern.as_os_str().to_string_lossy();
        let paths = glob::glob(&pattern);
        match paths {
            Ok(paths) => {
                let count = files.len();
                files.extend(paths.filter_map(Result::ok));
                if count == files.len() {
                    log::warn!("\"{}\" didn't match any files.", &pattern);
                }
            }
            Err(err) => {
                log::warn!(
                    "\"{}\" is not a valid pattern, and will be ignored. ({})",
                    &pattern,
                    &err.msg
                );
            }
        }
    }
    files
        .iter()
        .map(std::fs::canonicalize)
        .filter_map(Result::ok)
        .collect()
}

/// Loads a file to a string, handling non-utf-8 encoding
fn load_file(path: &PathBuf, _program: &CliArgs) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let encoding = chardet::detect(&bytes).0;
    let encoding = Encoding::for_label(encoding.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    Ok(encoding.decode(&bytes).0.to_string())
}

fn main() {
    // --- Process arguments and file list
    let mut opts = cli_args().run();
    // autodetect parallelism if set to 0
    if opts.jobs == 0 {
        opts.jobs = thread::available_parallelism()
            .unwrap_or(NonZeroUsize::new(1).unwrap())
            .into();
    }
    let opts = opts;
    // initialize logger based on chosen debug level
    if opts.verbose {
        pretty_env_logger::formatted_builder()
            .filter_level(Debug)
            .init();
    } else {
        pretty_env_logger::formatted_builder()
            .filter_level(Info)
            .init();
    }
    let paths = filter_paths(&opts.files);
    // if let Some(logpath) = &opts._logfile {
        // let file = File::create(logpath) else {
            // log::error!("Couldn't open {} for writing.", logpath)
        // };
    // }
    // make sure we have enough files
    if paths.len() <= 1 {
        log::error!("Got {} files to compare, need at least 2.", paths.len());
        return;
    } else {
        log::info!("Got {} files to compare.", paths.len())
    }

    // --- Compare files
    // preload all files into memory
    let mut files: HashMap<PathBuf, String> = HashMap::new();
    for path in &paths {
        files.insert(path.clone(), load_file(path, &opts).unwrap());
    }

    // hashmap for storing scores
    let mut scores: HashMap<(PathBuf, PathBuf), f64> = HashMap::new();

    // queue of comparisons that need to be made
    let mut workqueue: Vec<(&PathBuf, &PathBuf)> = Vec::new();
    for x in files.keys() {
        for y in files.keys() {
            // skip this comparison if we've already compared the two in opposite direction
            // or if it's the same file twice
            if x >= y {
                continue;
            }
            workqueue.push((x, y));
        }
    }

    let workqueue: Arc<Mutex<Vec<(&PathBuf, &PathBuf)>>> = Arc::new(Mutex::new(workqueue));
    // channel for receiving results

    // spawn the threads
    thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        let job_count = workqueue.lock().unwrap().len();
        // worker threads
        for x in 0..opts.jobs {
            let workqueue = workqueue.clone();
            let tx = tx.clone();
            // give the thread a name in case we have to debug specific threads later
            thread::Builder::new()
                .name(x.to_string())
                .spawn_scoped(scope, || work(workqueue, &files, tx))
                .unwrap();
        }
        // other thread
        scope.spawn(move || {
            let bar = ProgressBar::new(job_count as u64);
            // loop runs once per message from the worker threads (blocking while waiting)
            // and ends when all worker threads drop their Senders.
            for (x, y, score) in rx.iter() {
                scores.insert((x.clone(), y.clone()), score);
                if score >= opts.sensitivity {
                    // keep this import scoped small, otherwise everything gets
                    // a billion color methods in rust-analyzer.
                    use owo_colors::OwoColorize;
                    bar.println(format!(
                        "{:0>2.2}\t{}\t{}",
                        (score * 100.0).on_red(),
                        x.to_string_lossy(),
                        y.to_string_lossy(),
                    ));
                }
                bar.inc(1);
            }
            bar.finish();
        });
    });
}

/// Make comparisons until the workqueue is empty
fn work<'a>(
    jobs: Arc<Mutex<Vec<(&'a PathBuf, &'a PathBuf)>>>,
    files: &HashMap<PathBuf, String>,
    results: Sender<(&'a PathBuf, &'a PathBuf, f64)>,
) {
    let lev = eddie::str::Levenshtein::new();
    loop {
        // lock() blocks the thread, the Result is just for if the mutex is poisoned
        let job = jobs.lock().unwrap().pop();
        match job {
            None => break,
            Some((x, y)) => {
                let fx = files.get(x).unwrap();
                let fy = files.get(y).unwrap();
                let score = lev.similarity(fx, fy);
                let _ = results.send((x, y, score));
            }
        }
    }
    log::debug!(
        "Worker thread {} exited.",
        thread::current().name().unwrap()
    );
}

#[cfg(test)]
#[test]
fn check_opts() {
    cli_args().check_invariants(true);
}
