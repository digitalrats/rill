
//! Удобный реэкспорт основных типов.

pub use crate::traits::{
    AudioNode, Source, Processor, Sink,
    NodeId, NodeCategory, NodeMetadata, NodeTypeId,
    ParameterId, ParamValue, ParamType, ParamRange, ParamMetadata,
    PortId, PortType,
    AudioResult, AudioError,
};

pub use crate::signal::{
    Signal, SignalBus, BusConfig, OverflowPolicy,
    ParameterChanged, SignalSource, SystemEvent,
};