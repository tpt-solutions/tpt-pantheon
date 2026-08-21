//! tpt-telos parser: lexer, AST, and recursive-descent parser.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod span;

pub use ast::*;
pub use parser::parse;
pub use span::Span;

#[cfg(test)]
mod proptests {
    use super::*;

    /// Generate a well-formed minimal telos module that always parses.
    fn gen_telos_source(name: &str, func_name: &str, field: &str) -> String {
        format!(
            r#"module {name} {{
    invariant T {{ {field} >= 0 }}
    func {func_name}(x: T)
        requires {field} >= 0
        ensures {field} >= 0
    {{ }}
}}"#,
        )
    }

    #[test]
    fn generated_modules_parse() {
        let cases = vec![
            ("M1", "f1", "a"),
            ("M2", "f2", "balance"),
            ("Wallet", "deposit", "amount"),
            ("Bank", "transfer", "v"),
        ];
        for (mod_name, func_name, field) in cases {
            let src = gen_telos_source(mod_name, func_name, field);
            let result = parse(&src);
            assert!(
                result.is_ok(),
                "Failed to parse generated source for {mod_name}: {:?}\nSource:\n{src}",
                result.err()
            );
            let modules = result.unwrap();
            assert_eq!(modules.len(), 1);
            assert_eq!(modules[0].name, mod_name);
        }
    }

    #[test]
    fn disjunction_parses() {
        let src = r#"
            module M {
                func f(x: Int)
                    requires x == 1 || x == 2
                    ensures x >= 1 || x <= 0
                { }
            }
        "#;
        let result = parse(src);
        assert!(
            result.is_ok(),
            "disjunction should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn array_types_parse() {
        let src = r#"
            module M {
                func f(data: [Int; 5]) { }
            }
        "#;
        let result = parse(src);
        assert!(
            result.is_ok(),
            "array types should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn float_types_parse() {
        let src = r#"
            module M {
                func f(x: Float32, y: Float64) { }
            }
        "#;
        let result = parse(src);
        assert!(
            result.is_ok(),
            "float types should parse: {:?}",
            result.err()
        );
    }
}
