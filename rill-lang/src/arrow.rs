//! # The signal-arrow core
//!
//! A rill-lang program is an **arrow** (Hughes' *Arrows*) from n input channel
//! blocks to m output channel blocks:
//!
//! ```text
//! main : (I1:Block<Scalar> .. In:Block<Scalar>) -> (O1:Block<Scalar> .. Om:Block<Scalar>)
//! ```
//!
//! Three type levels:
//! - [`Scalar`](crate::types::ty::Scalar) — one sample (the element of a block);
//! - [`Block`](crate::types::ty::Block) — one signal channel: a fixed buffer of samples processed per tick;
//! - [`ArrowTy`](crate::types::ty::ArrowTy) — a block transform over channels.
//!
//! The block-diagram combinators are arrow laws. On every hardware tick the
//! backend executes the program: it feeds n input blocks through the arrow and
//! receives m output blocks. Stateful DSP (oscillators, filters, delay lines)
//! lives *inside* the arrow and persists between ticks.
//!
//! | Surface | `Expr` | Arrow law |
//! |---|---|---|
//! | `_` | `Expr::Wire` | identity `(X) -> (X)` |
//! | `!` | `Expr::Cut` | discard `(X) -> ()` |
//! | `A : B` | `Expr::Seq` | composition `out(A) == in(B)` |
//! | `A , B` | `Expr::Par` | product `(in A + in B, out A + out B)` |
//! | `A <: B` | `Expr::Split` | fan-out `in(B) = k·out(A)` |
//! | `A :> B` | `Expr::Merge` | fan-in sum `out(A) = k·in(B)` |
//! | `A ~ B` | `Expr::Loop` | 1-block delayed loop `in(B) ≤ out(A)`, `out(B) ≤ in(A)` |
//! | `A @ n` | `Expr::Delay` | block-level delay, `n` constant |
//! | `a + b` | `Expr::Arith` | elementwise |
//!
//! Fan-out/fan-in factors (`k`) are resolved at lowering from the inferred
//! channel arities (arity-indexed arrows).
//!
//! The arrow expressions are the **object level** of a two-level model. The
//! meta-level (Haskell-like) hosts definitions and applications; a definition's
//! [`Scheme`](crate::types::ty::Scheme) separates λ-parameters (meta-level) from signal channels
//! (object-level).
//!
//! The re-exports below are the arrow core and its type model.

pub use crate::ast::{ArithOp, Expr};
pub use crate::types::ty::{ArrowTy, Block, Scalar, Scheme};
