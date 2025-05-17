use std::error::Error;
use std::fmt;
use thiserror::Error;

use qp2p::EndpointError;

use crate::NodeID;

pub type P2PResult<T> = Result<T, P2PError>;

#[derive(Debug, Error)]
pub enum P2PError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerdeError(String),

    #[error("No connection ready: {nodeid}")]
    NoConnectionReady { nodeid: NodeID },

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Other error: {0}")]
    Other(String),

    #[error("Failed to start server: {0}")]
    StartServerError(EndpointError),

    #[error("Failed to deserialize message ID or task ID: {err}, context: {context}")]
    DeserialMsgIdTaskIdFailed {
        err: Box<bincode::ErrorKind>,
        context: String,
    },
}

// impl From<std::io::Error> for P2PError {
//     fn from(err: std::io::Error) -> Self {
//         P2PError::IoError(err)
//     }
// }

// impl From<Box<dyn Error + Send + Sync>> for P2PError {
//     fn from(err: Box<dyn Error + Send + Sync>) -> Self {
//         P2PError::Other(err.to_string())
//     }
// }
