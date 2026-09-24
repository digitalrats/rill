//! Arrow-category laws for the block-diagram combinators (see spec §2.3).
//!
//! A program is an arrow `(Block<Scalar>..) -> (Block<Scalar>..)`: a block
//! transform over channels. These tests assert the category identities over
//! arrow *types* (channel-count equations).

#[cfg(test)]
mod tests {
    use super::super::infer::par;
    use super::super::ty::{ArrowTy, Scalar};

    #[test]
    fn identity_is_unit() {
        // `_` : (X) -> (X) — the identity arrow.
        let id = ArrowTy::uniform(1, 1, Scalar::Float);
        assert_eq!((id.arity_in(), id.arity_out()), (1, 1));
    }

    #[test]
    fn parallel_adds_arities() {
        let a = ArrowTy::uniform(1, 2, Scalar::Float);
        let b = ArrowTy::uniform(3, 1, Scalar::Float);
        let p = par(&a, &b);
        assert_eq!((p.arity_in(), p.arity_out()), (4, 3));
    }

    #[test]
    fn block_wire_is_not_scalar() {
        // A channel is a Block, not a Scalar: arity counts channels.
        let t = ArrowTy::uniform(2, 2, Scalar::Float);
        assert_eq!(t.ins.len(), 2);
        assert!(t.ins.iter().all(|b| b.elem == Scalar::Float));
    }
}
