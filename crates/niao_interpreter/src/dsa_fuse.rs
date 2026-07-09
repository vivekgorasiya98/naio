//! AST-level DSA loop fusion for the tree-walking interpreter.
//!
//! Mirrors the bytecode patterns in `niao_vm::dsa_loops` so hot benchmark
//! loops run as single native passes without per-iteration dispatch.

use niao_ast::*;
use niao_runtime::dsa::fast;
use niao_runtime::{Environment, NativeDs, Value};
use std::cell::RefCell;
use std::rc::Rc;

type DsRef = Rc<RefCell<NativeDs>>;

/// If `cond`/`body` match a fused DSA loop, run it natively and return true.
pub fn try_run_while_fused(cond: &Expr, body: &Block, env: &Rc<Environment>) -> bool {
    try_fuse_count_push(cond, body, env)
        || try_fuse_map_build(cond, body, env)
        || try_fuse_map_lookup(cond, body, env)
        || try_fuse_drain_count(cond, body, env)
}

fn try_fuse_count_push(cond: &Expr, body: &Block, env: &Rc<Environment>) -> bool {
    let (i_var, limit_var) = match parse_lt_vars(cond) {
        Some(v) => v,
        None => return false,
    };
    if body.stmts.len() != 2 || parse_i_increment(body) != Some(i_var) {
        return false;
    }
    let Stmt::Expr(push_expr) = &body.stmts[0] else {
        return false;
    };
    let (fn_name, args) = match call_ident(push_expr) {
        Some(v) => v,
        None => return false,
    };
    let op = match fast::path_id(fn_name) {
        Some(op) if matches!(op, 0 | 3 | 6 | 11 | 16 | 19) => op,
        _ => return false,
    };
    if args.len() != 2 {
        return false;
    }
    let ds_name = match ident_name(&args[0]) {
        Some(v) => v,
        None => return false,
    };
    let (mul, add) = match parse_linear_arg(&args[1], i_var, env) {
        Some(v) => v,
        None => return false,
    };
    let ds = match env_native_ds(env, ds_name) {
        Some(v) => v,
        None => return false,
    };
    let i = match env_int(env, i_var) {
        Some(v) => v,
        None => return false,
    };
    let limit = match env_int(env, limit_var) {
        Some(v) => v,
        None => return false,
    };
    let end = match fast::fuse_count_push(&ds, op, i, limit, mul, add) {
        Some(v) => v,
        None => return false,
    };
    set_env_int(env, i_var, end);
    true
}

fn try_fuse_map_build(cond: &Expr, body: &Block, env: &Rc<Environment>) -> bool {
    let (i_var, limit_var) = match parse_lt_vars(cond) {
        Some(v) => v,
        None => return false,
    };
    if body.stmts.len() != 2 || parse_i_increment(body) != Some(i_var) {
        return false;
    }
    let Stmt::Expr(set_expr) = &body.stmts[0] else {
        return false;
    };
    let (fn_name, args) = match call_ident(set_expr) {
        Some(v) => v,
        None => return false,
    };
    if fn_name != "map_set" || args.len() != 3 {
        return false;
    }
    let ds_name = match ident_name(&args[0]) {
        Some(v) => v,
        None => return false,
    };
    if ident_name(&args[1]) != Some(i_var) {
        return false;
    }
    let mul = match &args[2] {
        Expr::Binary {
            left,
            op: BinOp::Mul,
            right,
            ..
        } if ident_name(left) == Some(i_var) => match right.as_ref() {
            Expr::Int(m, _) => *m,
            _ => return false,
        },
        _ => return false,
    };
    let ds = match env_native_ds(env, ds_name) {
        Some(v) => v,
        None => return false,
    };
    let i = match env_int(env, i_var) {
        Some(v) => v,
        None => return false,
    };
    let limit = match env_int(env, limit_var) {
        Some(v) => v,
        None => return false,
    };
    let end = match fast::fuse_map_build(&ds, i, limit, mul) {
        Some(v) => v,
        None => return false,
    };
    set_env_int(env, i_var, end);
    true
}

fn try_fuse_map_lookup(cond: &Expr, body: &Block, env: &Rc<Environment>) -> bool {
    let (i_var, limit_var) = match parse_lt_vars(cond) {
        Some(v) => v,
        None => return false,
    };
    if body.stmts.len() != 2 || parse_i_increment(body) != Some(i_var) {
        return false;
    }
    let (sum_var, ds_name, key_var) = match parse_map_lookup_assign(&body.stmts[0]) {
        Some(v) => v,
        None => return false,
    };
    if key_var != i_var {
        return false;
    }
    let ds = match env_native_ds(env, ds_name) {
        Some(v) => v,
        None => return false,
    };
    let i = match env_int(env, i_var) {
        Some(v) => v,
        None => return false,
    };
    let limit = match env_int(env, limit_var) {
        Some(v) => v,
        None => return false,
    };
    let sum = match env_int(env, sum_var) {
        Some(v) => v,
        None => return false,
    };
    let (end_i, end_sum) = match fast::fuse_map_lookup(&ds, i, limit, sum) {
        Some(v) => v,
        None => return false,
    };
    set_env_int(env, i_var, end_i);
    set_env_int(env, sum_var, end_sum);
    true
}

fn try_fuse_drain_count(cond: &Expr, body: &Block, env: &Rc<Environment>) -> bool {
    let inner = match cond {
        Expr::Unary {
            op: UnaryOp::Not,
            expr,
            ..
        } => expr.as_ref(),
        _ => return false,
    };
    let (empty_name, empty_args) = match call_ident(inner) {
        Some(v) => v,
        None => return false,
    };
    let empty_op = match fast::path_id(empty_name) {
        Some(op) if matches!(op, 5 | 18) => op,
        _ => return false,
    };
    if empty_args.len() != 1 {
        return false;
    }
    let ds_name = match ident_name(&empty_args[0]) {
        Some(v) => v,
        None => return false,
    };
    let pop_op = match empty_op {
        5 => 4,
        18 => 17,
        _ => return false,
    };
    if body.stmts.len() != 2 {
        return false;
    }
    let Stmt::Expr(pop_expr) = &body.stmts[0] else {
        return false;
    };
    let (pop_name, pop_args) = match call_ident(pop_expr) {
        Some(v) => v,
        None => return false,
    };
    if fast::path_id(pop_name) != Some(pop_op) || pop_args.len() != 1 {
        return false;
    }
    if ident_name(&pop_args[0]) != Some(ds_name) {
        return false;
    }
    let acc_var = match parse_acc_plus_one(&body.stmts[1]) {
        Some(v) => v,
        None => return false,
    };
    let ds = match env_native_ds(env, ds_name) {
        Some(v) => v,
        None => return false,
    };
    let acc = match env_int(env, acc_var) {
        Some(v) => v,
        None => return false,
    };
    let end = match fast::fuse_drain_count(&ds, pop_op, acc) {
        Some(v) => v,
        None => return false,
    };
    set_env_int(env, acc_var, end);
    true
}

fn call_ident<'a>(expr: &'a Expr) -> Option<(&'a str, &'a [Expr])> {
    if let Expr::Call { callee, args, .. } = expr {
        if let Expr::Ident(name, _) = callee.as_ref() {
            return Some((name.as_str(), args.as_slice()));
        }
    }
    None
}

fn ident_name(expr: &Expr) -> Option<&str> {
    if let Expr::Ident(name, _) = expr {
        Some(name.as_str())
    } else {
        None
    }
}

fn parse_lt_vars(cond: &Expr) -> Option<(&str, &str)> {
    if let Expr::Binary {
        left,
        op: BinOp::Lt,
        right,
        ..
    } = cond
    {
        Some((ident_name(left)?, ident_name(right)?))
    } else {
        None
    }
}

fn parse_i_increment(body: &Block) -> Option<&str> {
    let last = body.stmts.last()?;
    parse_acc_plus_one(last)
}

fn parse_acc_plus_one(stmt: &Stmt) -> Option<&str> {
    if let Stmt::Assign {
        target: AssignTarget::Name(name),
        op: AssignOp::Assign,
        value,
        ..
    } = stmt
    {
        if let Expr::Binary {
            left,
            op: BinOp::Add,
            right,
            ..
        } = value
        {
            if let Expr::Ident(acc, _) = left.as_ref() {
                if acc == name {
                    if let Expr::Int(1, _) = right.as_ref() {
                        return Some(name.as_str());
                    }
                }
            }
        }
    }
    None
}

fn parse_map_lookup_assign(stmt: &Stmt) -> Option<(&str, &str, &str)> {
    if let Stmt::Assign {
        target: AssignTarget::Name(sum_var),
        op: AssignOp::Assign,
        value,
        ..
    } = stmt
    {
        if let Expr::Binary {
            left,
            op: BinOp::Add,
            right,
            ..
        } = value
        {
            if let Expr::Ident(acc, _) = left.as_ref() {
                if acc != sum_var {
                    return None;
                }
                let (fn_name, args) = call_ident(right)?;
                if fn_name != "map_get" || args.len() != 2 {
                    return None;
                }
                return Some((
                    sum_var.as_str(),
                    ident_name(&args[0])?,
                    ident_name(&args[1])?,
                ));
            }
        }
    }
    None
}

fn parse_linear_arg(expr: &Expr, i_var: &str, env: &Rc<Environment>) -> Option<(i64, i64)> {
    if ident_name(expr) == Some(i_var) {
        return Some((1, 0));
    }
    if let Expr::Binary {
        left,
        op: BinOp::Mul,
        right,
        ..
    } = expr
    {
        if ident_name(left) == Some(i_var) {
            if let Expr::Int(m, _) = right.as_ref() {
                return Some((*m, 0));
            }
        }
    }
    if let Expr::Binary {
        left,
        op: BinOp::Sub,
        right,
        ..
    } = expr
    {
        if ident_name(right) == Some(i_var) {
            if let Expr::Ident(n_name, _) = left.as_ref() {
                let add = env_int(env, n_name)?;
                return Some((-1, add));
            }
            if let Expr::Int(n, _) = left.as_ref() {
                return Some((-1, *n));
            }
        }
    }
    if let Expr::Binary {
        left,
        op: BinOp::Add,
        right,
        ..
    } = expr
    {
        if ident_name(left) == Some(i_var) {
            if let Expr::Int(a, _) = right.as_ref() {
                return Some((1, *a));
            }
        }
    }
    None
}

fn env_int(env: &Environment, name: &str) -> Option<i64> {
    match &*env.get(name)?.borrow() {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

fn env_native_ds(env: &Environment, name: &str) -> Option<DsRef> {
    match &*env.get(name)?.borrow() {
        Value::Native(ds) => Some(Rc::clone(ds)),
        _ => None,
    }
}

fn set_env_int(env: &Environment, name: &str, n: i64) {
    env.assign(name, Value::Int(n).ref_cell());
}
