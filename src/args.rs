//! args.rs

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    #[arg(value_name = "SOURCE")]
    pub source: PathBuf,

    #[arg(value_name = "CONFIG")]
    pub config: PathBuf,

    #[arg(value_name = "DATA_DIR")]
    pub data_dir: PathBuf,
}

impl CliArgs {
    pub fn parse_args() -> Self {
        CliArgs::parse()
    }
}
