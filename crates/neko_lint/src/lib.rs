use neko_ast::*;
use neko_parser::parse;

#[derive(Debug, Clone)]
pub struct LintIssue {
    pub code: String,
    pub message: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug)]
pub enum LintError {
    Parse(neko_parser::ParseError),
}

impl std::fmt::Display for LintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LintError::Parse(e) => write!(f, "parse error: {e}"),
        }
    }
}

impl std::error::Error for LintError {}

impl From<neko_parser::ParseError> for LintError {
    fn from(e: neko_parser::ParseError) -> Self {
        LintError::Parse(e)
    }
}

pub fn lint_source(source: &str) -> Result<Vec<LintIssue>, LintError> {
    let program = parse(source)?;
    let mut issues = Vec::new();
    lint_program(&program, &mut issues);
    Ok(issues)
}

fn lint_program(program: &Program, issues: &mut Vec<LintIssue>) {
    let mut has_main = false;
    for item in &program.items {
        match item {
            TopLevel::Fn(f) => {
                if f.name == "main" {
                    has_main = true;
                }
                lint_fn(f, issues);
            }
            TopLevel::Route(r) => {
                lint_block(&r.body, issues);
                if r.path.is_empty() {
                    issues.push(LintIssue {
                        code: "W0001".into(),
                        message: "route path should not be empty".into(),
                        line: r.span.line,
                        col: r.span.col,
                    });
                }
            }
            TopLevel::Server(s) => {
                let has_port = s.fields.iter().any(|f| f.name == "port");
                if !has_port {
                    issues.push(LintIssue {
                        code: "W0002".into(),
                        message: "server block should define a port".into(),
                        line: s.span.line,
                        col: s.span.col,
                    });
                }
            }
            TopLevel::Stmt(s) => lint_stmt(s, issues),
            _ => {}
        }
    }
    // Script-style files (top-level statements) don't need a main function.
    if !has_main
        && !program
            .items
            .iter()
            .any(|i| matches!(i, TopLevel::Server(_) | TopLevel::Route(_) | TopLevel::Stmt(_)))
    {
        issues.push(LintIssue {
            code: "W0003".into(),
            message: "no main function defined".into(),
            line: program.span.line,
            col: program.span.col,
        });
    }
}

fn lint_fn(f: &FnDef, issues: &mut Vec<LintIssue>) {
    if f.name.starts_with('_') {
        issues.push(LintIssue {
            code: "W0004".into(),
            message: format!("function '{}' starts with underscore", f.name),
            line: f.span.line,
            col: f.span.col,
        });
    }
    lint_block(&f.body, issues);
}

fn lint_block(block: &Block, issues: &mut Vec<LintIssue>) {
    for stmt in &block.stmts {
        lint_stmt(stmt, issues);
    }
}

fn lint_stmt(stmt: &Stmt, issues: &mut Vec<LintIssue>) {
    match stmt {
        Stmt::VarDecl { init: None, span, .. } => {
            issues.push(LintIssue {
                code: "W0005".into(),
                message: "variable declared without initializer".into(),
                line: span.line,
                col: span.col,
            });
        }
        Stmt::If { then_block, else_block, .. } => {
            lint_block(then_block, issues);
            if else_block.is_none() {
                issues.push(LintIssue {
                    code: "W0006".into(),
                    message: "if statement without else".into(),
                    line: then_block.span.line,
                    col: then_block.span.col,
                });
            }
            if let Some(e) = else_block {
                lint_block(e, issues);
            }
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } => lint_block(body, issues),
        Stmt::Try { try_block, catch_block, .. } => {
            lint_block(try_block, issues);
            lint_block(catch_block, issues);
        }
        Stmt::Expr(expr) => lint_expr(expr, issues),
        _ => {}
    }
}

fn lint_expr(expr: &Expr, issues: &mut Vec<LintIssue>) {
    match expr {
        Expr::Binary { left, right, .. } => {
            lint_expr(left, issues);
            lint_expr(right, issues);
        }
        Expr::Unary { expr, .. } => lint_expr(expr, issues),
        Expr::Call { callee, args, .. } => {
            lint_expr(callee, issues);
            for arg in args {
                lint_expr(arg, issues);
            }
        }
        _ => {}
    }
}
