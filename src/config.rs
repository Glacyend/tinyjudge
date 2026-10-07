//! config.rs

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProblemConfig {
    pub name: String,

    #[serde(default = "default_time_limit_ms")]
    pub time_limit_ms: u64,

    #[serde(default = "default_memory_limit_mb")]
    pub memory_limit_mb: u64,

    #[serde(default = "default_input_suffix")]
    pub input_suffix: String,

    #[serde(default = "default_output_suffix")]
    pub output_suffix: String,

    #[serde(default)]
    pub data_prefix: String,

    pub testcase_count: usize,

    pub compile: CompileConfig,
}

fn default_time_limit_ms() -> u64 {
    1000
}

fn default_memory_limit_mb() -> u64 {
    256
}

fn default_input_suffix() -> String {
    String::from(".in")
}

fn default_output_suffix() -> String {
    String::from(".out")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileConfig {
    pub language: String,

    pub source_file: String,

    pub needs_compile: bool,

    #[serde(default)]
    pub compile_command: Option<String>,

    #[serde(default)]
    pub output_file: Option<String>,

    #[serde(default)]
    pub run_command: Option<String>,
}

impl CompileConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.needs_compile {
            if self.compile_command.is_none() {
                Err(ConfigError::Validation(String::from(
                    "`compile.compile_command` is required when `compile.needs_compile` is true.",
                )))
            } else if self.output_file.is_none() {
                Err(ConfigError::Validation(String::from(
                    "`compile.output_file` is required when `compile.needs_compile` is true.",
                )))
            } else {
                Ok(())
            }
        } else {
            if self.compile_command.is_some() {
                Err(ConfigError::Validation(String::from(
                    "`compile.compile_command` must not be set when `compile.needs_compile` is false.",
                )))
            } else if self.output_file.is_some() {
                Err(ConfigError::Validation(String::from(
                    "`compile.output_file` must not be set when `compile.needs_compile` is false.",
                )))
            } else if self.run_command.is_none() {
                Err(ConfigError::Validation(String::from(
                    "`compile.output_file` is required when `compile.needs_compile` is false.",
                )))
            } else {
                Ok(())
            }
        }
    }
}

impl ProblemConfig {
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path).map_err(|e| ConfigError::Read {
            path: path.to_path_buf(),
            source: e,
        })?;

        let config: ProblemConfig =
            serde_json::from_str(&content).map_err(|e| ConfigError::Parse {
                path: path.to_path_buf(),
                source: e,
            })?;

        config.compile.validate()?;

        Ok(config)
    }

    pub fn input_file_name(&self, index: usize) -> String {
        format!("{}{}{}", self.data_prefix, index, self.input_suffix)
    }

    pub fn output_file_name(&self, index: usize) -> String {
        format!("{}{}{}", self.data_prefix, index, self.output_suffix)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    Validation(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    f,
                    "failed to read config file {}: {}",
                    path.display(),
                    source
                )
            }
            Self::Parse { path, source } => {
                write!(
                    f,
                    "failed to parse config file {}: {}",
                    path.display(),
                    source
                )
            }
            Self::Validation(msg) => write!(f, "invalid config: {msg}"),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Validation(_) => None,
        }
    }
}
