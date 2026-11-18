use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("{t}{0}", t = crate::tr!("catalogue de formats invalide : ", "invalid format catalog: "))]
    Catalog(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
