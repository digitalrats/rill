//! # Router Trait — signal routing
//!
//! `Router` — a semantically separate type of graph node intended
//! exclusively for signal routing (mixers, matrix switches,
//! selectors). Unlike `Processor`, which performs DSP signal
//! processing, `Router` only redistributes input signals to outputs
//! with the ability to dynamically change connection topology.
//!
//! ## Differences from Processor
//!
//! | Characteristic | Processor | Router |
//! |---|---|---|
//! | I/O count | Fixed | Dynamic N→M |
//! | DSP | Yes (filter, effect) | No (only sum/commutation) |
//! | Topology | Known at build time | Can change at runtime |
//! | Visualization | Rectangle (P-scheme) | Diamond (P-scheme, condition) |

use crate::math::Transcendental;
use crate::time::RenderContext;
use crate::traits::node::Node;
use crate::traits::ProcessResult;

/// Signal router — N inputs, M outputs, configurable matrix.
///
/// Unlike `Processor::process()`, which performs DSP, `Router`
/// only redistributes input signals to outputs. The router
/// manages its own output ports via `Node::output_port_mut()`.
///
/// `TapeLoop` is obtained not through this trait, but through the graph resource registry
/// — see `GraphBuilder::add_resource()` and `Node::init()`.
pub trait Router<T: Transcendental, const BUF_SIZE: usize>: Node<T, BUF_SIZE> {
    /// Route one block.
    ///
    /// The implementation must read signals from `inputs` and write
    /// the results to its output ports (via `self.output_port_mut(i)`).
    fn route(&mut self, ctx: &RenderContext, inputs: &[&[T; BUF_SIZE]]) -> ProcessResult<()>;

    /// Number of input ports for routing.
    fn num_route_inputs(&self) -> usize;

    /// Number of output ports for routing.
    fn num_route_outputs(&self) -> usize;

    /// Set up a connection: route input `from` to output `to` with gain coefficient `gain`.
    fn set_connection(&mut self, from: usize, to: usize, gain: T) -> ProcessResult<()>;

    /// Remove a connection (zero the coefficient).
    fn remove_connection(&mut self, from: usize, to: usize) -> ProcessResult<()>;

    /// Get the current routing matrix: for each output — a list of inputs with gains.
    fn routing_matrix(&self) -> Vec<Vec<(usize, T)>>;
}
