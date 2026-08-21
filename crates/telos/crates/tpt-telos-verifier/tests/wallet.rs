use tpt_telos_ir::extract;
use tpt_telos_parser::parse;
use tpt_telos_verifier::verify;

#[test]
fn wallet_example_passes() {
    let src = std::fs::read_to_string("../../examples/wallet.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    for p in &problems {
        println!("FUNCTION: {}", p.func_name);
        println!("PREMISES:");
        for c in &p.premises {
            println!("  {:?}", c);
        }
        println!("CONCLUSIONS:");
        for c in &p.conclusions {
            println!(
                "  [{}] {}  ->  {:?}",
                c.is_ensures, c.description, c.constraint
            );
        }
    }
    // ensure it still verifies
    for p in &problems {
        let r = verify(p);
        assert!(r.all_passed, "expected all passed but got {:?}", r);
    }
}
