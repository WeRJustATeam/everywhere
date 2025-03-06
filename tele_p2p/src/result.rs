use std::error::Error;
use std::fmt;

pub type P2PResult<T> = Result<T, P2PError>;

#[derive(Debug)]
pub enum P2PError {
    IoError(std::io::Error),
    SerdeError(String),
    ConnectionError(String),
    InvalidMessage(String),
    Other(String),
}

impl fmt::Display for P2PError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            P2PError::IoError(e) => write!(f, "IO error: {}", e),
            P2PError::SerdeError(e) => write!(f, "Serialization error: {}", e),
            P2PError::ConnectionError(e) => write!(f, "Connection error: {}", e),
            P2PError::InvalidMessage(e) => write!(f, "Invalid message: {}", e),
            P2PError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl Error for P2PError {}

impl From<std::io::Error> for P2PError {
    fn from(err: std::io::Error) -> Self {
        P2PError::IoError(err)
    }
}

impl From<Box<dyn Error + Send + Sync>> for P2PError {
    fn from(err: Box<dyn Error + Send + Sync>) -> Self {
        P2PError::Other(err.to_string())
    }
} 