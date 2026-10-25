//! Host-owned state and runtime lifecycle. This crate never executes an agent on the host.
pub mod broker;
pub mod credentials;
pub mod model;
pub mod runtime;
pub mod service;
pub mod storage;
pub mod transport;
pub mod workspace;

pub use model::*;
pub use runtime::{Runtime, RuntimeAction, RuntimeObservation, SimulatorRuntime};
pub use service::ControlService;
pub use workspace::WorkspaceGrant;
