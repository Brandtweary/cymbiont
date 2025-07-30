use thiserror::Error;

pub mod logseq;
pub mod pkm_data;
pub mod import_utils;
pub mod reference_resolver;

pub use import_utils::import_logseq_graph;

#[derive(Error, Debug)]
pub enum ImportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
}

pub type Result<T> = std::result::Result<T, ImportError>;