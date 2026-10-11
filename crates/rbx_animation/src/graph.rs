//! Animation Graphs: Roblox's node-based way of driving a rig, authored as an
//! `AnimationGraphDefinition` and run as one animation track. Nodes blend,
//! mask, sequence and select clips, a state machine moves between states on
//! expressions over the graph's parameters, and the output node's pose is
//! what the rig shows.

pub mod dom;
pub mod expr;
pub mod model;
pub mod pose;
pub mod runtime;

pub use model::{Graph, Input, Node, NodeKind, Param, ParamKind, Prop, Transition, Value, WaitFor};
pub use runtime::Player;
