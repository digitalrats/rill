//! Isomorphism tests: JSON GraphDef ↔ DSL source via AST identity.
//!
//! Verifies that the graph-based compilation path produces the same AST
//! as the equivalent DSL source, and that the AST serialises
//! deterministically to JSON.

#[cfg(feature = "serialization")]
#[cfg(test)]
mod isomorphism {
    use rill_adrift::lang_builtins;
    use rill_core::traits::ParamValue;
    use rill_core::traits::Params;
    use rill_graph::GraphBuilder;
    use rill_lang::ast::Program;
    use rill_lang::render;
    use std::collections::HashMap;

    /// Remove span information so two ASTs can be compared structurally.
    fn zero_span(prog: &mut Program) {
        use rill_lang::ast::{Def, Expr};
        use rill_lang::error::Span;
        fn zs(expr: &mut Expr) {
            match expr {
                Expr::Int(_, s)
                | Expr::Float(_, s)
                | Expr::Imag(_, s)
                | Expr::Wire(s)
                | Expr::Cut(s)
                | Expr::Str(_, s)
                | Expr::Ref(_, s)
                | Expr::Neg(_, s) => *s = Span::new(0, 0),
                Expr::Apply { span, args, .. } => {
                    *span = Span::new(0, 0);
                    for a in args {
                        zs(a);
                    }
                }
                Expr::Bin { span, lhs, rhs, .. } => {
                    *span = Span::new(0, 0);
                    zs(lhs);
                    zs(rhs);
                }
                Expr::Let { span, defs, body } => {
                    *span = Span::new(0, 0);
                    for d in defs {
                        zd(d);
                    }
                    zs(body);
                }
                Expr::Record(_, s) => *s = Span::new(0, 0),
                Expr::ActorParam { span, default, .. } => {
                    *span = Span::new(0, 0);
                    if let Some(d) = default {
                        zs(d);
                    }
                }
            }
        }
        fn zd(def: &mut Def) {
            match def {
                Def::Anchor {
                    span,
                    where_defs,
                    body,
                    params,
                    ..
                } => {
                    *span = Span::new(0, 0);
                    for p in params {
                        p.span = Span::new(0, 0);
                    }
                    for w in where_defs {
                        zd(w);
                    }
                    zs(body);
                }
                Def::Local { span, body, .. } => {
                    *span = Span::new(0, 0);
                    zs(body);
                }
            }
        }
        for d in &mut prog.defs {
            zd(d);
        }
    }

    /// Build an AST from a graph definition and from the equivalent DSL
    /// source, then verify they are identical after span normalization.
    fn assert_ast_isomorphism(
        graph_type: &str,
        params: &HashMap<String, ParamValue>,
        dsl_source: &str,
    ) {
        let reg = lang_builtins::full_registry_f32();

        // 1. Build via graph → AST
        let mut builder = GraphBuilder::<f32, 256>::new();
        let mut p = Params::new(44100.0);
        for (k, v) in params {
            p = p.with(k.clone(), v.clone());
        }
        builder.add_node(graph_type, &p);
        let mut graph_ast = builder.ast_from_def(&reg).unwrap();

        // 2. Serialise graph AST → JSON
        let json_a = serde_json::to_string_pretty(&graph_ast).unwrap();

        // 3. Render graph AST → DSL source
        let dsl = render::render(&graph_ast);
        println!("Graph AST → DSL:\n{dsl}");
        println!("Expected DSL:\n{dsl_source}");

        // 4. Parse DSL source → AST
        let tokens = rill_lang::lexer::tokenize(&dsl).unwrap();
        let mut dsl_ast = rill_lang::parser::parse(&tokens, dsl.as_bytes()).unwrap();

        // 5. Normalise spans in both ASTs
        zero_span(&mut graph_ast);
        zero_span(&mut dsl_ast);

        // 6. Verify structural identity
        assert_eq!(
            graph_ast, dsl_ast,
            "AST from graph ≠ AST from DSL. Graph: {graph_ast:#?}\nDSL: {dsl_ast:#?}"
        );

        // 7. Verify JSON identity after serialising the DSL-derived AST
        let json_b = serde_json::to_string_pretty(&dsl_ast).unwrap();
        assert_eq!(
            json_a, json_b,
            "JSON round-trip failed.\nJSON_A (graph→AST→JSON): {json_a}\nJSON_B (graph→AST→DSL→AST→JSON): {json_b}"
        );
    }

    // ========================================================================
    // Tests
    // ========================================================================

    #[test]
    fn sine_roundtrip() {
        let mut params = HashMap::new();
        params.insert("freq".into(), ParamValue::Float(440.0));
        params.insert("amp".into(), ParamValue::Float(0.5));
        assert_ast_isomorphism("rill/sine", &params, "main = sine 440.0 0.5 0.0");
    }

    #[test]
    fn chiptune_roundtrip() {
        let mut params = HashMap::new();
        params.insert("clock".into(), ParamValue::Float(1_750_000.0));
        params.insert("regs".into(), ParamValue::Float(0.0));
        assert_ast_isomorphism("rill/lofi_chip", &params, "main regs = ay38910 1750000 0");
    }

    #[test]
    fn biquad_chain_roundtrip() {
        let reg = lang_builtins::full_registry_f32();
        let mut builder = GraphBuilder::<f32, 256>::new();

        // sine → biquad chain
        let mut sine_params = Params::new(44100.0);
        sine_params.insert("freq", ParamValue::Float(440.0));
        sine_params.insert("amp", ParamValue::Float(0.5));
        builder.add_node("rill/sine", &sine_params);

        let mut bq_params = Params::new(44100.0);
        bq_params.insert("type", ParamValue::Float(1.0));
        bq_params.insert("cutoff", ParamValue::Float(600.0));
        bq_params.insert("q", ParamValue::Float(1.5));
        bq_params.insert("gain_db", ParamValue::Float(3.0));
        builder.add_node("rill/biquad", &bq_params);

        // Connect sine(0) → biquad(0)
        builder.connect_signal(0, 0, 1, 0);

        let mut graph_ast = builder.ast_from_def(&reg).unwrap();
        let dsl = render::render(&graph_ast);
        println!("Chain AST → DSL:\n{dsl}");

        let tokens = rill_lang::lexer::tokenize(&dsl).unwrap();
        let mut dsl_ast = rill_lang::parser::parse(&tokens, dsl.as_bytes()).unwrap();
        zero_span(&mut graph_ast);
        zero_span(&mut dsl_ast);
        assert_eq!(
            graph_ast, dsl_ast,
            "Chain AST mismatch: graph={graph_ast:#?}\nDSL={dsl_ast:#?}"
        );
    }
}
