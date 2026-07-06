use neko_ast::*;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct IrModule {
    pub functions: Vec<IrFunction>,
    pub constants: Vec<IrConst>,
    pub classes: Vec<ClassDef>,
    pub traits: Vec<TraitDef>,
    pub field_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IrFunction {
    pub name: String,
    pub params: Vec<String>,
    pub instructions: Vec<IrInstr>,
}

#[derive(Debug, Clone)]
pub enum IrConst {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nil,
}

#[derive(Debug, Clone)]
pub enum IrInstr {
    Const(usize),
    Load(String),
    Store(String),
    Binary(BinOp),
    Unary(UnaryOp),
    Call { name: String, argc: usize },
    Return,
    Jump(usize),
    JumpIfFalse(usize),
    Pop,
    MakeArray(usize),
    MakeObject(Vec<usize>),
    MakeInstance { class: String, field_count: usize },
    GetField(usize),
    SetField(usize),
    GetIndex,
    SetIndex,
    CallMethod { field: usize, argc: usize },
    CallStatic { class: String, method: String, argc: usize },
    CallSuper { method: String, argc: usize },
    BindGlobal(String),
    TryBegin(usize),
    TryEnd(usize),
    Throw,
}

struct LoopCtx {
    breaks: Vec<usize>,
    continue_target: usize,
}

#[derive(Debug)]
pub enum IrError {
    Unsupported(String),
}

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrError::Unsupported(msg) => write!(f, "unsupported top-level item for IR: {msg}"),
        }
    }
}

impl std::error::Error for IrError {}

/// Name of the synthetic function that holds top-level script statements.
pub const TOPLEVEL_FN: &str = "__toplevel__";

fn field_idx(field_names: &mut Vec<String>, field: &str) -> usize {
    if let Some(i) = field_names.iter().position(|n| n == field) {
        i
    } else {
        field_names.push(field.to_string());
        field_names.len() - 1
    }
}

pub fn lower(program: &Program) -> Result<IrModule, IrError> {
    let class_names: HashSet<String> = program
        .items
        .iter()
        .filter_map(|item| match item {
            TopLevel::Class(c) => Some(c.name.clone()),
            _ => None,
        })
        .collect();
    let mut constants = Vec::new();
    let mut functions = Vec::new();
    let mut has_main = false;
    let mut top_instrs = Vec::new();
    let mut top_loops = Vec::new();
    let mut classes = Vec::new();
    let mut traits = Vec::new();
    let mut field_names: Vec<String> = Vec::new();

    for item in &program.items {
        match item {
            TopLevel::Trait(t) => traits.push(t.clone()),
            TopLevel::Class(c) => {
                classes.push(c.clone());
                for member in &c.members {
                    if let ClassMember::Method { def, .. } | ClassMember::StaticMethod { def, .. } =
                        member
                    {
                        let mut instrs = Vec::new();
                        let mut loops = Vec::new();
                        lower_block(
                            &def.body,
                            &mut instrs,
                            &mut constants,
                            &mut loops,
                            &mut field_names,
                            &class_names,
                            false,
                        )?;
                        functions.push(IrFunction {
                            name: if matches!(member, ClassMember::StaticMethod { .. }) {
                                format!("__CS__{}__{}", c.name, def.name)
                            } else {
                                mangle_class_fn(c, def)
                            },
                            params: def.params.iter().map(|p| p.name.clone()).collect(),
                            instructions: instrs,
                        });
                    }
                }
            }
            TopLevel::Fn(f) => {
                if f.name == "main" {
                    has_main = true;
                }
                let mut instrs = Vec::new();
                let mut loops = Vec::new();
                lower_block(
                    &f.body,
                    &mut instrs,
                    &mut constants,
                    &mut loops,
                    &mut field_names,
                    &class_names,
                    false,
                )?;
                functions.push(IrFunction {
                    name: f.name.clone(),
                    params: f.params.iter().map(|p| p.name.clone()).collect(),
                    instructions: instrs,
                });
            }
            TopLevel::Import(imp) => {
                if let Some(export) = neko_runtime::native_module_export_name(&imp.path) {
                    if let Some(alias) = &imp.alias {
                        if alias != export {
                            top_instrs.push(IrInstr::Load(export.to_string()));
                            top_instrs.push(IrInstr::BindGlobal(alias.clone()));
                        }
                    }
                }
            }
            TopLevel::Stmt(stmt) => {
                lower_stmt(
                    stmt,
                    &mut top_instrs,
                    &mut constants,
                    &mut top_loops,
                    &mut field_names,
                    &class_names,
                    true,
                )?;
            }
            _ => {}
        }
    }

    if !top_instrs.is_empty() {
        if has_main {
            top_instrs.push(IrInstr::Call {
                name: "main".into(),
                argc: 0,
            });
            top_instrs.push(IrInstr::Pop);
        }
        functions.push(IrFunction {
            name: TOPLEVEL_FN.to_string(),
            params: Vec::new(),
            instructions: top_instrs,
        });
    }

    Ok(IrModule {
        functions,
        constants,
        classes,
        traits,
        field_names,
    })
}

fn mangle_class_fn(class: &ClassDef, def: &FnDef) -> String {
    format!("__C__{}__{}", class.name, def.name)
}

fn const_idx(constants: &mut Vec<IrConst>, c: IrConst) -> usize {
    let idx = constants.len();
    constants.push(c);
    idx
}

fn lower_block(
    block: &Block,
    instrs: &mut Vec<IrInstr>,
    constants: &mut Vec<IrConst>,
    loops: &mut Vec<LoopCtx>,
    field_names: &mut Vec<String>,
    class_names: &HashSet<String>,
    module_scope: bool,
) -> Result<(), IrError> {
    for stmt in &block.stmts {
        lower_stmt(
            stmt,
            instrs,
            constants,
            loops,
            field_names,
            class_names,
            module_scope,
        )?;
    }
    Ok(())
}

fn lower_stmt(
    stmt: &Stmt,
    instrs: &mut Vec<IrInstr>,
    constants: &mut Vec<IrConst>,
    loops: &mut Vec<LoopCtx>,
    field_names: &mut Vec<String>,
    class_names: &HashSet<String>,
    module_scope: bool,
) -> Result<(), IrError> {
    match stmt {
        Stmt::VarDecl { name, init, .. } => {
            if let Some(expr) = init {
                lower_expr(expr, instrs, constants, field_names, class_names)?;
            } else {
                instrs.push(IrInstr::Const(const_idx(constants, IrConst::Nil)));
            }
            if module_scope {
                instrs.push(IrInstr::BindGlobal(name.clone()));
            } else {
                instrs.push(IrInstr::Store(name.clone()));
            }
        }
        Stmt::Assign { target, op, value, .. } => match target {
            AssignTarget::Name(name) => {
                match op {
                    AssignOp::Assign => lower_expr(value, instrs, constants, field_names, class_names)?,
                    AssignOp::AddAssign => {
                        instrs.push(IrInstr::Load(name.clone()));
                        lower_expr(value, instrs, constants, field_names, class_names)?;
                        instrs.push(IrInstr::Binary(BinOp::Add));
                    }
                    AssignOp::SubAssign => {
                        instrs.push(IrInstr::Load(name.clone()));
                        lower_expr(value, instrs, constants, field_names, class_names)?;
                        instrs.push(IrInstr::Binary(BinOp::Sub));
                    }
                }
                instrs.push(IrInstr::Store(name.clone()));
            }
            AssignTarget::Index { object, index } => {
                lower_expr(object, instrs, constants, field_names, class_names)?;
                lower_expr(index, instrs, constants, field_names, class_names)?;
                lower_expr(value, instrs, constants, field_names, class_names)?;
                instrs.push(IrInstr::SetIndex);
            }
            AssignTarget::Member { object, field } => {
                lower_expr(object, instrs, constants, field_names, class_names)?;
                lower_expr(value, instrs, constants, field_names, class_names)?;
                instrs.push(IrInstr::SetField(field_idx(field_names, field)));
            }
        },
        Stmt::Expr(expr) => {
            lower_expr(expr, instrs, constants, field_names, class_names)?;
            instrs.push(IrInstr::Pop);
        }
        Stmt::Return { value, .. } => {
            if let Some(expr) = value {
                lower_expr(expr, instrs, constants, field_names, class_names)?;
            } else {
                instrs.push(IrInstr::Const(const_idx(constants, IrConst::Nil)));
            }
            instrs.push(IrInstr::Return);
        }
        Stmt::If { cond, then_block, else_block, .. } => {
            lower_expr(cond, instrs, constants, field_names, class_names)?;
            let jump_false = instrs.len();
            instrs.push(IrInstr::JumpIfFalse(0));
            lower_block(
                then_block,
                instrs,
                constants,
                loops,
                field_names,
                class_names,
                module_scope,
            )?;
            if let Some(else_blk) = else_block {
                let jump_end = instrs.len();
                instrs.push(IrInstr::Jump(0));
                let else_start = instrs.len();
                if let IrInstr::JumpIfFalse(ref mut target) = instrs[jump_false] {
                    *target = else_start;
                }
                lower_block(
                    else_blk,
                    instrs,
                    constants,
                    loops,
                    field_names,
                    class_names,
                    module_scope,
                )?;
                let end = instrs.len();
                if let IrInstr::Jump(ref mut target) = instrs[jump_end] {
                    *target = end;
                }
            } else {
                let end = instrs.len();
                if let IrInstr::JumpIfFalse(ref mut target) = instrs[jump_false] {
                    *target = end;
                }
            }
        }
        Stmt::While { cond, body, .. } => {
            let loop_start = instrs.len();
            lower_expr(cond, instrs, constants, field_names, class_names)?;
            let jump_false = instrs.len();
            instrs.push(IrInstr::JumpIfFalse(0));
            loops.push(LoopCtx {
                breaks: Vec::new(),
                continue_target: loop_start,
            });
            lower_block(
                body,
                instrs,
                constants,
                loops,
                field_names,
                class_names,
                module_scope,
            )?;
            let ctx = loops.pop().expect("loop context");
            instrs.push(IrInstr::Jump(loop_start));
            let end = instrs.len();
            if let IrInstr::JumpIfFalse(ref mut target) = instrs[jump_false] {
                *target = end;
            }
            for break_idx in ctx.breaks {
                if let IrInstr::Jump(ref mut target) = instrs[break_idx] {
                    *target = end;
                }
            }
        }
        Stmt::Break(_) => {
            let jump = instrs.len();
            instrs.push(IrInstr::Jump(0));
            loops
                .last_mut()
                .expect("break outside loop")
                .breaks
                .push(jump);
        }
        Stmt::Continue(_) => {
            let target = loops
                .last()
                .expect("continue outside loop")
                .continue_target;
            instrs.push(IrInstr::Jump(target));
        }
        Stmt::Try {
            try_block,
            catch_var,
            catch_block,
            ..
        } => {
            let try_begin = instrs.len();
            instrs.push(IrInstr::TryBegin(0));
            lower_block(
                try_block,
                instrs,
                constants,
                loops,
                field_names,
                class_names,
                module_scope,
            )?;
            let try_end = instrs.len();
            instrs.push(IrInstr::TryEnd(0));
            let catch_start = instrs.len();
            if let IrInstr::TryBegin(ref mut target) = instrs[try_begin] {
                *target = catch_start;
            }
            instrs.push(IrInstr::Store(catch_var.clone()));
            lower_block(
                catch_block,
                instrs,
                constants,
                loops,
                field_names,
                class_names,
                module_scope,
            )?;
            let after = instrs.len();
            if let IrInstr::TryEnd(ref mut target) = instrs[try_end] {
                *target = after;
            }
        }
        Stmt::Throw { value, .. } => {
            lower_expr(value, instrs, constants, field_names, class_names)?;
            instrs.push(IrInstr::Throw);
        }
        _ => {}
    }
    Ok(())
}

fn lower_expr(
    expr: &Expr,
    instrs: &mut Vec<IrInstr>,
    constants: &mut Vec<IrConst>,
    field_names: &mut Vec<String>,
    class_names: &HashSet<String>,
) -> Result<(), IrError> {
    match expr {
        Expr::Int(v, _) => instrs.push(IrInstr::Const(const_idx(constants, IrConst::Int(*v)))),
        Expr::Float(v, _) => instrs.push(IrInstr::Const(const_idx(constants, IrConst::Float(*v)))),
        Expr::String(v, _) => instrs.push(IrInstr::Const(const_idx(
            constants,
            IrConst::String(v.clone()),
        ))),
        Expr::Bool(v, _) => instrs.push(IrInstr::Const(const_idx(constants, IrConst::Bool(*v)))),
        Expr::Nil(_) => instrs.push(IrInstr::Const(const_idx(constants, IrConst::Nil))),
        Expr::Ident(name, _) => instrs.push(IrInstr::Load(name.clone())),
        Expr::Binary { left, op, right, .. } => {
            lower_expr(left, instrs, constants, field_names, class_names)?;
            lower_expr(right, instrs, constants, field_names, class_names)?;
            instrs.push(IrInstr::Binary(*op));
        }
        Expr::Unary { op, expr, .. } => {
            lower_expr(expr, instrs, constants, field_names, class_names)?;
            instrs.push(IrInstr::Unary(*op));
        }
        Expr::Call { callee, args, .. } => match &**callee {
            Expr::Ident(name, _) => {
                for arg in args {
                    lower_expr(arg, instrs, constants, field_names, class_names)?;
                }
                instrs.push(IrInstr::Call {
                    name: name.clone(),
                    argc: args.len(),
                });
            }
            Expr::Member { object, field, .. } => {
                if let Expr::Ident(class_name, _) = &**object {
                    if class_names.contains(class_name) {
                        for arg in args {
                            lower_expr(arg, instrs, constants, field_names, class_names)?;
                        }
                        instrs.push(IrInstr::CallStatic {
                            class: class_name.clone(),
                            method: field.clone(),
                            argc: args.len(),
                        });
                        return Ok(());
                    }
                }
                lower_expr(object, instrs, constants, field_names, class_names)?;
                for arg in args {
                    lower_expr(arg, instrs, constants, field_names, class_names)?;
                }
                instrs.push(IrInstr::CallMethod {
                    field: field_idx(field_names, field),
                    argc: args.len(),
                });
            }
            _ => {}
        },
        Expr::Object { fields, .. } => {
            let mut indices = Vec::with_capacity(fields.len());
            for (name, expr) in fields {
                indices.push(field_idx(field_names, name));
                lower_expr(expr, instrs, constants, field_names, class_names)?;
            }
            instrs.push(IrInstr::MakeObject(indices));
        }
        Expr::ClassInit { name, fields, .. } => {
            for (_, expr) in fields {
                lower_expr(expr, instrs, constants, field_names, class_names)?;
            }
            instrs.push(IrInstr::MakeInstance {
                class: name.clone(),
                field_count: fields.len(),
            });
        }
        Expr::StructInit { name, fields, .. } => {
            for (_, expr) in fields {
                lower_expr(expr, instrs, constants, field_names, class_names)?;
            }
            instrs.push(IrInstr::MakeInstance {
                class: name.clone(),
                field_count: fields.len(),
            });
        }
        Expr::SuperCall { method, args, .. } => {
            for arg in args {
                lower_expr(arg, instrs, constants, field_names, class_names)?;
            }
            instrs.push(IrInstr::CallSuper {
                method: method.clone(),
                argc: args.len(),
            });
        }
        Expr::Array { elements, .. } => {
            for el in elements {
                lower_expr(el, instrs, constants, field_names, class_names)?;
            }
            instrs.push(IrInstr::MakeArray(elements.len()));
        }
        Expr::Member { object, field, .. } => {
            lower_expr(object, instrs, constants, field_names, class_names)?;
            instrs.push(IrInstr::GetField(field_idx(field_names, field)));
        }
        Expr::Index { object, index, .. } => {
            lower_expr(object, instrs, constants, field_names, class_names)?;
            lower_expr(index, instrs, constants, field_names, class_names)?;
            instrs.push(IrInstr::GetIndex);
        }
    }
    Ok(())
}
