use std::collections::HashMap;
use std::sync::Arc;

/// A named function that can be referenced from serialized MappingDef
/// and ServoDef instead of a raw closure.
///
/// Each function takes an input value plus a parameter map and returns a mapped value.
pub type NamedFunction = Arc<dyn Fn(f64, &HashMap<String, f64>) -> f64 + Send + Sync>;

/// Registry of named functions for serialization-safe custom transforms.
///
/// Provides the bridge between the non-serializable `Transform::Custom(Arc<dyn Fn>)`
/// and the serializable `TransformDef::NamedFunction { name, params }`.
///
/// # Example
///
/// ```
/// use rill_patchbay::function_registry::FunctionRegistry;
///
/// let reg = FunctionRegistry::builtin();
/// let out = reg.apply("tanh", 0.5, &Default::default()).unwrap();
/// assert!((out - 0.5f64.tanh()).abs() < 1e-10);
/// ```
#[derive(Clone)]
pub struct FunctionRegistry {
    functions: HashMap<String, NamedFunction>,
}

impl FunctionRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    /// Register a named function.
    pub fn register(
        &mut self,
        name: impl Into<String>,
        f: NamedFunction,
    ) {
        self.functions.insert(name.into(), f);
    }

    /// Apply a named function.
    ///
    /// Returns `None` if the function name is not registered.
    pub fn apply(&self, name: &str, input: f64, params: &HashMap<String, f64>) -> Option<f64> {
        self.functions.get(name).map(|f| f(input, params))
    }

    /// Fill with built-in functions.
    pub fn builtin() -> Self {
        let mut reg = Self::new();

        reg.register("tanh", Arc::new(|x, _| x.tanh()));
        reg.register("clip", Arc::new(|x, p| {
            let lo = p.get("min").copied().unwrap_or(-1.0);
            let hi = p.get("max").copied().unwrap_or(1.0);
            x.clamp(lo, hi)
        }));
        reg.register("scale", Arc::new(|x, p| {
            let from_lo = p.get("from_min").copied().unwrap_or(0.0);
            let from_hi = p.get("from_max").copied().unwrap_or(1.0);
            let to_lo = p.get("to_min").copied().unwrap_or(0.0);
            let to_hi = p.get("to_max").copied().unwrap_or(1.0);
            let norm = (x - from_lo) / (from_hi - from_lo);
            to_lo + norm * (to_hi - to_lo)
        }));
        reg.register("invert", Arc::new(|x, _| 1.0 - x));
        reg.register("abs", Arc::new(|x, _| x.abs()));
        reg.register("smooth", Arc::new(|x, p| {
            let factor = p.get("factor").copied().unwrap_or(0.5);
            x * factor
            // Note: true smoothing requires state (one-pole), handled at runtime
        }));
        reg.register("quantize", Arc::new(|x, p| {
            let steps = p.get("steps").copied().unwrap_or(12.0);
            (x * steps).round() / steps
        }));

        reg
    }
}

impl Default for FunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_tanh() {
        let reg = FunctionRegistry::builtin();
        let params = HashMap::new();
        let out = reg.apply("tanh", 0.5, &params).unwrap();
        assert!((out - 0.5f64.tanh()).abs() < 1e-10);
    }

    #[test]
    fn test_builtin_clip() {
        let reg = FunctionRegistry::builtin();
        let mut params = HashMap::new();
        params.insert("min".into(), -0.5);
        params.insert("max".into(), 0.5);
        let out = reg.apply("clip", 2.0, &params).unwrap();
        assert!((out - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_builtin_scale() {
        let reg = FunctionRegistry::builtin();
        let mut params = HashMap::new();
        params.insert("from_min".into(), 0.0);
        params.insert("from_max".into(), 1.0);
        params.insert("to_min".into(), 0.0);
        params.insert("to_max".into(), 127.0);
        let out = reg.apply("scale", 0.5, &params).unwrap();
        assert!((out - 63.5).abs() < 1e-10);
    }

    #[test]
    fn test_unknown_function() {
        let reg = FunctionRegistry::new();
        assert!(reg.apply("nonexistent", 0.0, &HashMap::new()).is_none());
    }
}
