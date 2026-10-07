//! main.rs

use std::{path::PathBuf, process};

use crate::{
    args::CliArgs,
    config::ProblemConfig,
    judge::{CompileOutcome, JudgeError, TestcaseVerdict, Verdict},
};

mod args;
mod config;
mod judge;

fn main() {
    let cli = CliArgs::parse_args();

    let config = match ProblemConfig::from_file(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    println!("Problem name: {}", config.name);
    println!("Time limit: {} ms", config.time_limit_ms);
    println!("Memory limit: {} MB", config.memory_limit_mb);
    println!("Input suffix: {}", config.input_suffix);
    println!("Output suffix: {}", config.output_suffix);
    println!("Data prefix: {:?}", config.data_prefix);
    println!("Test case count: {}", config.testcase_count);

    println!("Language: {}", config.compile.language);
    println!("Configured source name: {}", config.compile.source_file);
    println!("Needs compile: {}", config.compile.needs_compile);
    if let Some(cmd) = &config.compile.compile_command {
        println!("Compile command: {cmd}");
    }
    if let Some(out) = &config.compile.output_file {
        println!("Output file: {out}");
    }
    if let Some(cmd) = &config.compile.run_command {
        println!("Run command: {cmd}");
    }

    println!("Data directory: {}", cli.data_dir.display());
    println!("Source file: {}", cli.source.display());

    println!();

    match run_judge(&cli, &config) {
        Ok(verdict) => match verdict {
            Verdict::Accepted => process::exit(0),
            Verdict::CompileError | Verdict::Unaccepted => process::exit(1),
        },
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(2);
        }
    }
}

fn run_judge(cli: &CliArgs, config: &ProblemConfig) -> Result<Verdict, JudgeError> {
    judge::check_source_file_name(&cli.source, config)?;
    judge::check_data_files(&cli.data_dir, config)?;

    let output_path = if config.compile.needs_compile {
        Some(judge::output_path(&cli.source, config))
    } else {
        None
    };

    let outcome = judge::compile(&cli.source, config)?;

    let executable: PathBuf = match &outcome {
        CompileOutcome::Failed(message) => {
            eprint!("{message}");
            if let Some(path) = output_path {
                judge::cleanup_output(&path);
            }
            println!("Verdict: {}", Verdict::CompileError);
            return Ok(Verdict::CompileError);
        }
        CompileOutcome::NotNeeded => cli.source.clone(),
        CompileOutcome::Success(path) => {
            println!("Compiled to: {}", path.display());
            path.clone()
        }
    };

    let verdicts = judge::run_testcases(config, &cli.data_dir, &executable)?;

    if let Some(path) = output_path {
        judge::cleanup_output(&path);
    }

    let scores = judge::distribute_scores(config.testcase_count);
    let mut total_score: u32 = 0;
    let mut all_accepted = true;

    println!();
    for (index, verdict) in verdicts.iter().enumerate() {
        match verdict {
            TestcaseVerdict::Accepted => {
                let score = scores[index];
                total_score += score;
                println!("Test case {}: Accepted ({} points)", index, score);
            }
            TestcaseVerdict::WrongAnswer(detail) => {
                all_accepted = false;
                println!("Test case {}: Wrong Answer (0 points)", index);
                println!("  First mismatch at line {}:", detail.line_number);
                match &detail.expected {
                    Some(line) => {
                        println!("    Expected: \"{}\"", judge::preview_line(line))
                    }
                    None => println!("    Expected: <no line>"),
                }
                match &detail.actual {
                    Some(line) => println!("    Actual:   \"{}\"", judge::preview_line(line)),
                    None => println!("    Actual:   <no line>"),
                }
            }
            TestcaseVerdict::RuntimeError { exit_code } => {
                all_accepted = false;
                match exit_code {
                    Some(code) => println!(
                        "Test case {}: Runtime Error (exit code: {}) (0 points)",
                        index, code
                    ),
                    None => println!(
                        "Test case {}: Runtime Error (terminated by signal) (0 points)",
                        index
                    ),
                }
            }
            TestcaseVerdict::TimeLimitExceeded => {
                all_accepted = false;
                println!("Test case {}: Time Limit Exceeded (0 points)", index);
            }
            TestcaseVerdict::MemoryLimitExceeded { peak_mb } => {
                all_accepted = false;
                match peak_mb {
                    Some(mb) => println!(
                        "Test case {}: Memory Limit Exceeded ({} MB) (0 points)",
                        index, mb
                    ),
                    None => println!(
                        "Test case {}: Memory Limit Exceeded (virtual memory) (0 points)",
                        index
                    ),
                }
            }
        }
    }

    let final_verdict = if all_accepted {
        Verdict::Accepted
    } else {
        Verdict::Unaccepted
    };

    println!();
    println!("Verdict: {final_verdict}");
    println!("Score: {total_score} / 100");

    Ok(final_verdict)
}
