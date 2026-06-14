//! Graph constructor — creates a signal graph from a [`GraphDef`] descriptor
//! and returns its actor handle.
//!
//! The graph runs on a dedicated OS thread (its I/O callback drives processing).
//! [`GraphConstructor`] captures the node and backend factories at construction
//! time so they do not need to be passed on every call.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core_actor::{ActorRef, ActorSystem};

use crate::serialization::GraphDef;
use crate::{GraphBuilder, NodeFactory};

/// Error returned by [`GraphConstructor::run`].
#[derive(Debug, Clone)]
pub enum GraphBuildError {
    /// Build failed with a user-readable reason.
    BuildFailed(String),
}

/// Captures factories and builds [`crate::Graph`] instances from [`GraphDef`].
pub struct GraphConstructor<T: Transcendental, const BUF: usize> {
    node_factory: Arc<Mutex<NodeFactory<T, BUF>>>,
}

impl<T: Transcendental, const BUF: usize> GraphConstructor<T, BUF> {
    /// Create a new graph constructor with a shared node factory.
    pub fn new(node_factory: Arc<Mutex<NodeFactory<T, BUF>>>) -> Self {
        Self { node_factory }
    }

    /// Build a signal graph from `def` and return the actor handles.
    ///
    /// # Arguments
    /// * `def` — graph topology descriptor
    /// * `system` — actor system for spawning the inline drain actor
    /// * `parent_ref` — parent actor for the graph to send `ClockTick` to
    ///
    /// Returns a tuple of `(graph_thread, graph_handle)` where
    /// `graph_thread` is a join handle for the dedicated I/O thread,
    /// and `graph_handle` is the `ActorRef<CommandEnum>` that receives
    /// parameter commands.
    #[allow(clippy::type_complexity)]
    pub fn run(
        &self,
        def: &GraphDef,
        system: &Arc<ActorSystem>,
        parent_ref: ActorRef<CommandEnum>,
        running: Arc<AtomicBool>,
    ) -> Result<(std::thread::JoinHandle<()>, ActorRef<CommandEnum>), GraphBuildError> {
        let nf = Arc::clone(&self.node_factory);
        let def = def.clone();
        let sys = Arc::clone(system);

        let (graph_tx, graph_rx) = std::sync::mpsc::channel();

        let handle = std::thread::spawn(move || {
            let mut builder = GraphBuilder::new(Arc::new(nf.lock().unwrap().clone()));
            builder.set_parent_ref(parent_ref);
            if let Err(e) = def.populate(&mut builder) {
                log::error!("graph populate: {e}");
                return;
            }
            match builder.build(&sys) {
                Ok(mut graph) => {
                    let _ = graph_tx.send(graph.handle());
                    graph.run(running).ok();
                }
                Err(e) => log::error!("graph build: {e:?}"),
            }
        });

        let graph_ref = graph_rx
            .recv()
            .map_err(|e| GraphBuildError::BuildFailed(format!("graph handle: {e}")))?;

        Ok((handle, graph_ref))
    }
}
