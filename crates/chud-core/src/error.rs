use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("catalogue de formats invalide : {0}")]
    Catalog(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
