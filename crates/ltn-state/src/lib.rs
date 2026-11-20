//! Shared base for the ltn runtime crates. Frozen during the sprint: other
//! crates build on these types and must not need to change them.

pub mod docker;
pub mod spec;
pub mod ssh;
pub mod store;
pub mod tool;

pub use ltn_core::solver::placement::{
    place, PlacementDecision, PlacementError, PlacementRequest, TargetCapacity,
};
pub use tool::{Ctx, Handler, Offer, Provisioner, ToolDef};
