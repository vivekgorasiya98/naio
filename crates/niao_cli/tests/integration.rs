use niao_interpreter::Interpreter;
use niao_parser::parse;
use std::path::PathBuf;

fn run_example(name: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name);
    let mut interp = Interpreter::new().with_base_dir(
        path.parent().unwrap().to_path_buf(),
    );
    interp.run_file(&path).expect(&format!("failed to run {name}"));
}

fn run_test(name: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join(name);
    let mut interp = Interpreter::new().with_base_dir(
        path.parent().unwrap().to_path_buf(),
    );
    interp.run_file(&path).expect(&format!("failed to run {name}"));
}

#[test]
fn example_hello() {
    run_example("hello.niao");
}

#[test]
fn example_fibonacci() {
    run_example("fibonacci.niao");
}

#[test]
fn example_factorial() {
    run_example("factorial.niao");
}

#[test]
fn example_loops() {
    run_example("loops.niao");
}

#[test]
fn example_structs() {
    run_example("structs.niao");
}

#[test]
fn example_oop_basics() {
    run_example("oop_basics.niao");
}

#[test]
fn example_oop_inheritance() {
    run_example("oop_inheritance.niao");
}

#[test]
fn example_oop_traits() {
    run_example("oop_traits.niao");
}

#[test]
fn test_oop_classes() {
    run_test("oop_classes.niao");
}

#[test]
fn test_oop_traits() {
    run_test("oop_traits.niao");
}

#[test]
fn test_oop_vm() {
    run_test("oop_vm.niao");
}

#[test]
fn example_errors() {
    run_example("errors.niao");
}

#[test]
fn example_import_demo() {
    run_example("import_demo.niao");
}

#[test]
fn test_basic() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("basic.niao");
    let mut interp = Interpreter::new().with_base_dir(
        path.parent().unwrap().to_path_buf(),
    );
    interp.run_file(&path).unwrap();
}

#[test]
fn parses_web_server() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("web_server.niao");
    let source = std::fs::read_to_string(&path).unwrap();
    let program = parse(&source).unwrap();
    assert!(program.items.len() >= 3);
}

#[test]
fn format_roundtrip() {
    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join("hello.niao"),
    )
    .unwrap();
    let formatted = niao_format::format_source(&source).unwrap();
    assert!(formatted.contains("fn greet"));
    assert!(parse(&formatted).is_ok());
}

#[test]
fn test_nsqlite() {
    run_test("nsqlite.niao");
}

#[test]
fn example_nsqlite_demo() {
    run_example("nsqlite_demo.niao");
}

#[test]
fn test_npg() {
    run_test("npg.niao");
}

#[test]
fn example_npg_demo() {
    run_example("npg_demo.niao");
}

#[test]
fn lint_hello() {
    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join("hello.niao"),
    )
    .unwrap();
    let issues = niao_lint::lint_source(&source).unwrap();
    assert!(issues.iter().any(|i| i.code == "W0006") || issues.is_empty());
}

#[test]
fn build_bytecode() {
    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join("hello.niao"),
    )
    .unwrap();
    let program = parse(&source).unwrap();
    let bc = niao_bytecode::compile_to_bytecode(&program).unwrap();
    assert!(!bc.functions.is_empty());
}

#[test]
fn docs_generation() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("hello.niao");
    let source = std::fs::read_to_string(&path).unwrap();
    let html = niao_docs::generate_docs(&source, &path).unwrap();
    assert!(html.contains("fn greet"));
}
