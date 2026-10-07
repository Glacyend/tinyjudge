//! judge.rs

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::config::ProblemConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    CompileError,
    Accepted,
    Unaccepted,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::CompileError => write!(f, "Compile Error"),
            Verdict::Accepted => write!(f, "Accepted"),
            Verdict::Unaccepted => write!(f, "Unaccepted"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestcaseVerdict {
    Accepted,
    WrongAnswer(WrongAnswerDetail),
    RuntimeError { exit_code: Option<i32> },
    TimeLimitExceeded,
    MemoryLimitExceeded { peak_mb: Option<u64> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrongAnswerDetail {
    pub line_number: usize,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

impl fmt::Display for TestcaseVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestcaseVerdict::Accepted => write!(f, "Accepted"),
            TestcaseVerdict::WrongAnswer(_) => write!(f, "Wrong Answer"),
            TestcaseVerdict::RuntimeError { .. } => write!(f, "Runtime Error"),
            TestcaseVerdict::TimeLimitExceeded => write!(f, "Time Limit Exceeded"),
            TestcaseVerdict::MemoryLimitExceeded { .. } => write!(f, "Memory Limit Exceeded"),
        }
    }
}

#[derive(Debug)]
pub enum JudgeError {
    InvalidSourcePath(PathBuf),
    SourceNameMismatch { expected: String, actual: String },
    CommandTemplate(String),
    DataFiles(Vec<String>),
    Io(io::Error),
}

impl fmt::Display for JudgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSourcePath(p) => write!(f, "invalid source path: {}", p.display()),
            Self::SourceNameMismatch { expected, actual } => write!(
                f,
                "source file name mismatch: expected `{expected}`, got `{actual}`."
            ),
            Self::DataFiles(problems) => {
                write!(f, "data files check failed:")?;
                for problem in problems {
                    write!(f, "\n  - {problem}")?;
                }
                Ok(())
            }
            Self::CommandTemplate(msg) => write!(f, "invalid command template; {msg}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl Error for JudgeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for JudgeError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug)]
pub enum CompileOutcome {
    NotNeeded,
    Success(PathBuf),
    Failed(String),
}

pub fn output_path(source: &Path, config: &ProblemConfig) -> PathBuf {
    let output_name = config
        .compile
        .output_file
        .as_ref()
        .expect("`output_file` is present when `needs_compile` is true.");
    match source.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(output_name),
        _ => PathBuf::from(output_name),
    }
}

pub fn cleanup_output(path: &Path) {
    if !path.exists() {
        return;
    }
    if let Err(e) = fs::remove_file(path) {
        eprintln!(
            "Warning: failed to remove output file {}: {}",
            path.display(),
            e
        );
    }
}

pub fn check_source_file_name(source: &Path, config: &ProblemConfig) -> Result<(), JudgeError> {
    let actual = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| JudgeError::InvalidSourcePath(source.to_path_buf()))?
        .to_string();

    if actual != config.compile.source_file {
        return Err(JudgeError::SourceNameMismatch {
            expected: config.compile.source_file.clone(),
            actual,
        });
    }

    Ok(())
}

pub fn check_data_files(data_dir: &Path, config: &ProblemConfig) -> Result<(), JudgeError> {
    let mut problems: Vec<String> = Vec::new();

    for index in 0..config.testcase_count {
        let input = data_dir.join(config.input_file_name(index));
        let output = data_dir.join(config.output_file_name(index));
        if !input.is_file() {
            problems.push(format!("missing input file: {}", input.display()));
        }
        if !output.is_file() {
            problems.push(format!("missing output file: {}", output.display()));
        }
    }

    for entry in fs::read_dir(data_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        let index = match (
            extract_testcase_index(name, &config.data_prefix, &config.input_suffix),
            extract_testcase_index(name, &config.data_prefix, &config.output_suffix),
        ) {
            (IndexMatch::NoMatch, IndexMatch::NoMatch) => continue,
            (IndexMatch::Match(n), _) | (_, IndexMatch::Match(n)) => n,
            _ => {
                problems.push(format!("malformed data file name: {name}"));
                continue;
            }
        };

        if index >= config.testcase_count {
            problems.push(format!(
                "unexpected data file (test index {index} out of range 0..{}): {name}",
                config.testcase_count
            ));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(JudgeError::DataFiles(problems))
    }
}

enum IndexMatch {
    NoMatch,
    Match(usize),
    Malformed,
}

fn extract_testcase_index(name: &str, prefix: &str, suffix: &str) -> IndexMatch {
    let rest = match name.strip_prefix(prefix) {
        Some(rest) => rest,
        None => return IndexMatch::NoMatch,
    };
    let digits = match rest.strip_suffix(suffix) {
        Some(digits) => digits,
        None => return IndexMatch::NoMatch,
    };
    match digits.parse::<usize>() {
        Ok(index) => IndexMatch::Match(index),
        Err(_) => IndexMatch::Malformed,
    }
}

pub fn compile(source: &Path, config: &ProblemConfig) -> Result<CompileOutcome, JudgeError> {
    if !config.compile.needs_compile {
        return Ok(CompileOutcome::NotNeeded);
    }

    let template =
        config.compile.compile_command.as_ref().expect(
            "validate() guarantees `compile_command` is present when needs_compile is true.",
        );

    let output_path = output_path(source, config);

    let command_line = substitute_placeholders(template, Some(source), &output_path, "compile")?;

    let output = Command::new("sh")
        .arg("-c")
        .arg(&command_line)
        .output()
        .map_err(JudgeError::Io)?;

    if output.status.success() {
        Ok(CompileOutcome::Success(output_path))
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut message = String::new();
        if !stdout.is_empty() {
            message.push_str("--- compiler stdout ---\n");
            message.push_str(&stdout);
            if !stdout.ends_with('\n') {
                message.push('\n');
            }
        }
        if !stderr.is_empty() {
            message.push_str("--- compiler stderr ---\n");
            message.push_str(&stderr);
            if !stderr.ends_with('\n') {
                message.push('\n');
            }
        }
        if message.is_empty() {
            message.push_str("compiler exited with a non-zero status but produced no output.\n");
        }

        Ok(CompileOutcome::Failed(message))
    }
}

enum RunOutcome {
    Completed {
        status: Option<ExitStatus>,
        cpu: Duration,
        peak_rss_bytes: u64,
    },
    VirtualMemoryExceeded,
}

#[cfg(unix)]
fn run_timed(
    command: &mut Command,
    hard_limit: Duration,
    memory_limit_bytes: u64,
) -> Result<RunOutcome, JudgeError> {
    use std::os::unix::process::CommandExt;

    // Restrict the child's virtual address space. This makes `exec` fail
    // with ENOMEM when a program declares more memory than the limit --
    // including a huge uninitialized BSS, which never shows up in RSS.
    let rlim = memory_limit_bytes as libc::rlim_t;
    unsafe {
        command.pre_exec(move || {
            let limit = libc::rlimit {
                rlim_cur: rlim,
                rlim_max: rlim,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.raw_os_error() == Some(libc::ENOMEM) => {
            // `exec` failed because the virtual memory limit was too small.
            return Ok(RunOutcome::VirtualMemoryExceeded);
        }
        Err(e) => return Err(JudgeError::Io(e)),
    };
    let pid = child.id() as libc::pid_t;
    drop(child);

    let start = Instant::now();
    loop {
        let mut raw_status: libc::c_int = 0;
        let mut rusage: libc::rusage = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::wait4(pid, &mut raw_status, libc::WNOHANG, &mut rusage) };

        if ret == pid {
            use std::os::unix::process::ExitStatusExt;
            return Ok(RunOutcome::Completed {
                status: Some(ExitStatus::from_raw(raw_status)),
                cpu: rusage_cpu(&rusage),
                peak_rss_bytes: rusage_rss_bytes(&rusage),
            });
        }

        if ret < 0 {
            let err = io::Error::last_os_error();
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                let mut s: libc::c_int = 0;
                libc::wait4(pid, &mut s, 0, std::ptr::null_mut());
            }
            return Err(JudgeError::Io(err));
        }

        let now = start.elapsed();
        if now >= hard_limit {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            let mut final_status: libc::c_int = 0;
            let mut final_rusage: libc::rusage = unsafe { std::mem::zeroed() };
            unsafe { libc::wait4(pid, &mut final_status, 0, &mut final_rusage) };
            return Ok(RunOutcome::Completed {
                status: None,
                cpu: rusage_cpu(&final_rusage),
                peak_rss_bytes: rusage_rss_bytes(&final_rusage),
            });
        }

        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(not(unix))]
fn run_timed(
    _command: &mut Command,
    _hard_limit: Duration,
    _memory_limit_bytes: u64,
) -> Result<RunOutcome, JudgeError> {
    Err(JudgeError::Io(io::Error::new(
        io::ErrorKind::Unsupported,
        "resource measurement is only implemented on Unix",
    )))
}

#[cfg(unix)]
fn rusage_cpu(rusage: &libc::rusage) -> Duration {
    let secs = rusage.ru_utime.tv_sec as u64 + rusage.ru_stime.tv_sec as u64;
    let usecs = rusage.ru_utime.tv_usec as u64 + rusage.ru_stime.tv_usec as u64;
    Duration::from_secs(secs) + Duration::from_micros(usecs)
}

#[cfg(unix)]
fn rusage_rss_bytes(rusage: &libc::rusage) -> u64 {
    let raw = rusage.ru_maxrss as u64;
    // Linux and most Unix report `ru_maxrss` in kibibytes; macOS reports bytes.
    #[cfg(target_os = "macos")]
    {
        raw
    }
    #[cfg(not(target_os = "macos"))]
    {
        raw * 1024
    }
}

pub fn run_testcases(
    config: &ProblemConfig,
    data_dir: &Path,
    executable: &Path,
) -> Result<Vec<TestcaseVerdict>, JudgeError> {
    let temp_path = std::env::temp_dir().join(format!(
        "oj_judge_{}_{}.tmp",
        std::process::id(),
        config.name
    ));

    let _ = fs::remove_file(&temp_path);

    let result = (|| -> Result<Vec<TestcaseVerdict>, JudgeError> {
        let mut verdicts = Vec::with_capacity(config.testcase_count);
        for index in 0..config.testcase_count {
            let input_path = data_dir.join(config.input_file_name(index));
            let expected_path = data_dir.join(config.output_file_name(index));

            let mut command = build_run_command(config, executable)?;

            let stdin_file = fs::File::open(&input_path)?;
            let stdout_file = fs::File::create(&temp_path)?;
            command
                .stdin(Stdio::from(stdin_file))
                .stdout(Stdio::from(stdout_file))
                .stderr(Stdio::null());

            let time_limit = Duration::from_millis(config.time_limit_ms);
            let hard_limit = time_limit + Duration::from_millis(200);
            let memory_limit_bytes = config.memory_limit_mb * 1024 * 1024;

            let outcome = run_timed(&mut command, hard_limit, memory_limit_bytes)?;

            let verdict = match outcome {
                RunOutcome::VirtualMemoryExceeded => {
                    TestcaseVerdict::MemoryLimitExceeded { peak_mb: None }
                }
                RunOutcome::Completed {
                    status,
                    cpu,
                    peak_rss_bytes,
                } => {
                    let peak_mb = peak_rss_bytes / (1024 * 1024);
                    if peak_rss_bytes > memory_limit_bytes {
                        TestcaseVerdict::MemoryLimitExceeded {
                            peak_mb: Some(peak_mb),
                        }
                    } else if status.is_none() || cpu > time_limit {
                        TestcaseVerdict::TimeLimitExceeded
                    } else if let Some(status) = status {
                        if status.success() {
                            let actual = fs::read_to_string(&temp_path)?;
                            let expected = fs::read_to_string(&expected_path)?;
                            match compare_output(&actual, &expected) {
                                Some(detail) => TestcaseVerdict::WrongAnswer(detail),
                                None => TestcaseVerdict::Accepted,
                            }
                        } else {
                            TestcaseVerdict::RuntimeError {
                                exit_code: status.code(),
                            }
                        }
                    } else {
                        unreachable!("`status` is always `Some` unless the process was killed")
                    }
                }
            };
            verdicts.push(verdict);
        }
        Ok(verdicts)
    })();

    let _ = fs::remove_file(&temp_path);
    result
}

pub fn distribute_scores(count: usize) -> Vec<u32> {
    if count == 0 {
        return Vec::new();
    }
    let base = 100 / count as u32;
    let extra = 100 % count as u32;
    (0..count as u32)
        .map(|i| if i < extra { base + 1 } else { base })
        .collect()
}

fn build_run_command(config: &ProblemConfig, executable: &Path) -> Result<Command, JudgeError> {
    match &config.compile.run_command {
        Some(template) => {
            let replaced = substitute_placeholders(template, None, executable, "run")?;
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(&replaced);
            Ok(cmd)
        }
        None => Ok(Command::new(executable)),
    }
}

fn normalize_lines(s: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = s.lines().map(|line| line.trim_end()).collect();
    while matches!(lines.last(), Some(line) if line.is_empty()) {
        lines.pop();
    }
    lines
}

fn compare_output(actual: &str, expected: &str) -> Option<WrongAnswerDetail> {
    let actual_lines = normalize_lines(actual);
    let expected_lines = normalize_lines(expected);

    let max = actual_lines.len().max(expected_lines.len());
    for i in 0..max {
        let a = actual_lines.get(i).copied();
        let e = expected_lines.get(i).copied();
        if a != e {
            return Some(WrongAnswerDetail {
                line_number: i + 1,
                expected: e.map(str::to_string),
                actual: a.map(str::to_string),
            });
        }
    }
    None
}

pub fn preview_line(line: &str) -> String {
    const LIMIT: usize = 10;
    let mut chars = line.chars();
    let prefix: String = chars.by_ref().take(LIMIT).collect();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}

fn substitute_placeholders(
    template: &str,
    input: Option<&Path>,
    output: &Path,
    context: &str,
) -> Result<String, JudgeError> {
    let mut result = String::new();
    let mut rest = template;

    while let Some(pos) = rest.find("${") {
        result.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];

        let end = after.find('}').ok_or_else(|| {
            JudgeError::CommandTemplate(format!(
                "unterminated placeholder in {context} command: {template}"
            ))
        })?;

        let name = &after[..end];
        match name {
            "INPUT_FILE" => match input {
                Some(path) => {
                    result.push('"');
                    result.push_str(&path.display().to_string());
                    result.push('"');
                }
                None => {
                    return Err(JudgeError::CommandTemplate(format!(
                        "`${{INPUT_FILE}}` is not supported in {context} command: {template}"
                    )));
                }
            },
            "OUTPUT_FILE" => {
                result.push('"');
                result.push_str(&output.display().to_string());
                result.push('"');
            }
            other => {
                return Err(JudgeError::CommandTemplate(format!(
                    "unknown placeholder `${{{other}}}` in {context} command: {template}"
                )));
            }
        }

        rest = &after[end + 1..];
    }

    result.push_str(rest);
    Ok(result)
}
