use thiserror::Error;

#[derive(Error, Debug)]
pub enum TestEnvError {
    #[error("deployment failed: {0}")]
    DeployFailed(String),

    #[error("call reverted: {0}")]
    Reverted(String),

    #[error("execution halted: {0}")]
    Halted(String),

    #[error("deployment did not return an address")]
    NoAddress,
}
