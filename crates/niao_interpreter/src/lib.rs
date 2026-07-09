use niao_ast::*;
use niao_parser::parse;
use niao_runtime::*;

mod dsa_fuse;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

static GLOBAL_INTERP: Mutex<Option<usize>> = Mutex::new(None);

#[derive(Debug)]
pub enum InterpreterError {
    Runtime(RuntimeError),
    Parse(niao_parser::ParseError),
    Io(std::io::Error),
}

impl std::fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterpreterError::Runtime(e) => write!(f, "{e}"),
            InterpreterError::Parse(e) => write!(f, "parse error: {e}"),
            InterpreterError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for InterpreterError {}

impl From<RuntimeError> for InterpreterError {
    fn from(e: RuntimeError) -> Self {
        InterpreterError::Runtime(e)
    }
}

impl From<niao_parser::ParseError> for InterpreterError {
    fn from(e: niao_parser::ParseError) -> Self {
        InterpreterError::Parse(e)
    }
}

impl From<std::io::Error> for InterpreterError {
    fn from(e: std::io::Error) -> Self {
        InterpreterError::Io(e)
    }
}

pub struct Interpreter {
    globals: Rc<Environment>,
    structs: HashMap<String, StructDef>,
    class_registry: Rc<RefCell<ClassRegistry>>,
    module_loader: ModuleLoader,
    base_dir: PathBuf,
    stdlib_dir: Option<PathBuf>,
    /// Class body currently executing (for private access checks).
    current_class: Option<String>,
}

enum ExecResult {
    Value(ValueRef),
    Return(ValueRef),
    Break,
    Continue,
}

impl Interpreter {
    pub fn new() -> Self {
        let globals = Environment::child(builtin_environment());
        let class_registry = Rc::new(RefCell::new(ClassRegistry::new()));
        set_class_registry(Rc::clone(&class_registry));
        Self {
            globals,
            structs: HashMap::new(),
            class_registry,
            module_loader: ModuleLoader::new(),
            base_dir: PathBuf::from("."),
            stdlib_dir: None,
            current_class: None,
        }
    }

    pub fn with_base_dir(mut self, dir: PathBuf) -> Self {
        self.base_dir = dir;
        self
    }

    pub fn with_stdlib_dir(mut self, dir: PathBuf) -> Self {
        self.stdlib_dir = Some(dir);
        self
    }

    pub fn set_base_dir(&mut self, dir: PathBuf) {
        self.base_dir = dir;
    }

    pub fn run_source(&mut self, source: &str) -> Result<ValueRef, InterpreterError> {
        let program = parse(source)?;
        self.with_call_hook(|this| this.execute_program(&program))
    }

    pub fn run_file(&mut self, path: &Path) -> Result<ValueRef, InterpreterError> {
        let source = fs::read_to_string(path)?;
        self.base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let program = parse(&source)?;
        self.with_call_hook(|this| this.execute_program(&program))
    }

    /// Run entry file and keep the Niao call hook active (for long-running servers).
    pub fn run_file_keep_hook(&mut self, path: &Path) -> Result<ValueRef, InterpreterError> {
        let source = fs::read_to_string(path)?;
        self.base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let program = parse(&source)?;
        self.enable_call_hook();
        self.execute_program(&program)
    }

    /// Install the interpreter call hook until `disable_call_hook` is called.
    pub fn enable_call_hook(&mut self) {
        self.install_call_hook();
    }

    pub fn disable_call_hook(&mut self) {
        self.clear_call_hook();
    }

    pub fn with_call_hook<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce(&mut Self) -> T,
    {
        self.install_call_hook();
        let result = f(self);
        self.clear_call_hook();
        result
    }

    /// Look up a top-level function registered in the interpreter globals.
    pub fn lookup_global_function(&self, name: &str) -> Option<ValueRef> {
        self.globals.get(name)
    }

    fn install_call_hook(&mut self) {
        let ptr = self as *mut Self;
        *GLOBAL_INTERP.lock().unwrap() = Some(ptr as usize);
        set_niao_fn_resolver(Some(Arc::new(|name| {
            let guard = GLOBAL_INTERP.lock().unwrap();
            let interp_ptr = (*guard.as_ref()?) as *const Interpreter;
            let interp = unsafe { &*interp_ptr };
            interp.lookup_global_function(name)
        })));
        set_niao_call_hook(Some(Arc::new(|callee, args, span| {
            let guard = GLOBAL_INTERP.lock().unwrap();
            let interp_ptr = guard.ok_or_else(|| {
                RuntimeError::at(
                    span,
                    codes::E1404_NET_HTTP,
                    "interpreter call hook not active",
                )
            })? as *mut Interpreter;
            let interp = unsafe { &mut *interp_ptr };
            interp.invoke_function(callee, args, span)
        })));
    }

    fn clear_call_hook(&mut self) {
        set_niao_call_hook(None);
        set_niao_fn_resolver(None);
        *GLOBAL_INTERP.lock().unwrap() = None;
    }

    /// Invoke a Niao callable from native code (HTTP server handlers).
    pub fn invoke_function(
        &mut self,
        callee: ValueRef,
        args: &[ValueRef],
        span: Span,
    ) -> Result<ValueRef, RuntimeError> {
        self.call_value(callee, args, span)
    }

    fn execute_program(&mut self, program: &Program) -> Result<ValueRef, InterpreterError> {
        let mut trait_defs = Vec::new();
        let mut class_defs = Vec::new();

        for item in &program.items {
            match item {
                TopLevel::Import(imp) => {
                    self.load_module(imp)?;
                }
                TopLevel::Struct(s) => {
                    self.structs.insert(s.name.clone(), s.clone());
                }
                TopLevel::Trait(t) => trait_defs.push(t.clone()),
                TopLevel::Class(c) => class_defs.push(c.clone()),
                TopLevel::Fn(f) => {
                    self.define_function(f, Rc::clone(&self.globals))?;
                }
                TopLevel::Server(_) | TopLevel::Route(_) | TopLevel::Stmt(_) => {}
            }
        }

        for t in &trait_defs {
            self.class_registry
                .borrow_mut()
                .register_trait(t)
                .map_err(InterpreterError::Runtime)?;
        }
        for c in &class_defs {
            self.register_class(c)?;
        }

        let result = self.run_top_level(program);
        flush_print_buffer();
        result
    }

    fn register_class(&mut self, def: &ClassDef) -> Result<(), InterpreterError> {
        let globals = Rc::clone(&self.globals);
        let make_fn = |fdef: &FnDef, closure: Rc<Environment>| FunctionValue {
            def: fdef.clone(),
            closure,
        };
        self.class_registry
            .borrow_mut()
            .finalize_class(def, &make_fn, globals)
            .map_err(InterpreterError::Runtime)?;
        Ok(())
    }

    fn current_class_name(&self) -> Option<&str> {
        self.current_class.as_deref()
    }

    fn run_top_level(&mut self, program: &Program) -> Result<ValueRef, InterpreterError> {
        let globals = Rc::clone(&self.globals);
        let mut last = Value::Nil.ref_cell();
        for item in &program.items {
            if let TopLevel::Stmt(stmt) = item {
                match self.execute_stmt(stmt, Rc::clone(&globals))? {
                    ExecResult::Return(v) => return Ok(v),
                    ExecResult::Value(v) => last = v,
                    ExecResult::Break | ExecResult::Continue => {
                        return Err(InterpreterError::Runtime(RuntimeError::at(
                            program.span,
                            1005,
                            "break/continue outside loop",
                        )));
                    }
                }
            }
        }

        if let Some(main_fn) = self.globals.get("main") {
            self.call_value(main_fn, &[], Span::dummy())
                .map_err(InterpreterError::Runtime)
        } else {
            Ok(last)
        }
    }

    fn load_module(&mut self, imp: &ImportStmt) -> Result<(), InterpreterError> {
        let import_path = &imp.path;
        // Native modules ship inside the runtime; their functions are already
        // registered as builtins, so the import succeeds with no file lookup.
        if niao_runtime::native_module_paths().contains(&import_path.trim_matches('"')) {
            if let Some(export) = niao_runtime::native_module_export_name(import_path) {
                if let Some(val) = self.globals.get(export) {
                    if let Some(alias) = &imp.alias {
                        self.globals
                            .define(alias.clone(), Rc::clone(&val));
                    }
                }
            }
            return Ok(());
        }

        let resolved = self.resolve_module_path(import_path);
        let key = resolved.to_string_lossy().to_string();

        if self.module_loader.modules.contains_key(&key) {
            let module = Rc::clone(self.module_loader.modules.get(&key).unwrap());
            self.import_exports(&module);
            return Ok(());
        }

        if self.module_loader.loading.contains(&key) {
            return Err(InterpreterError::Runtime(RuntimeError::ImportCycle {
                path: key,
            }));
        }

        self.module_loader.loading.push(key.clone());
        let source = fs::read_to_string(&resolved).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                let path = import_path.trim_matches('"');
                let hint = if matches!(path, "nmongo" | "std/nmongo") {
                    " (nmongo is not enabled in this niao build)"
                } else {
                    ""
                };
                InterpreterError::Runtime(RuntimeError::ModuleNotFound {
                    path: format!("{}{}", resolved.display(), hint),
                })
            } else {
                InterpreterError::Io(e)
            }
        })?;
        let program = parse(&source)?;

        let saved_base = self.base_dir.clone();
        self.base_dir = resolved.parent().unwrap_or(Path::new(".")).to_path_buf();

        let mut exports = HashMap::new();
        let mut module_structs = HashMap::new();
        let mut module_classes = HashMap::new();
        let mut module_traits = HashMap::new();
        let mut trait_defs = Vec::new();
        let mut class_defs = Vec::new();

        for item in &program.items {
            match item {
                TopLevel::Import(imp) => {
                    self.load_module(imp)?;
                }
                TopLevel::Struct(s) => {
                    module_structs.insert(s.name.clone(), s.clone());
                    self.structs.insert(s.name.clone(), s.clone());
                }
                TopLevel::Trait(t) => {
                    module_traits.insert(t.name.clone(), t.clone());
                    trait_defs.push(t.clone());
                }
                TopLevel::Class(c) => {
                    module_classes.insert(c.name.clone(), c.clone());
                    class_defs.push(c.clone());
                }
                TopLevel::Fn(f) => {
                    self.define_function(f, Rc::clone(&self.globals))?;
                    if let Some(val) = self.globals.get(&f.name) {
                        exports.insert(f.name.clone(), val);
                    }
                }
                _ => {}
            }
        }

        for t in &trait_defs {
            self.class_registry
                .borrow_mut()
                .register_trait(t)
                .map_err(InterpreterError::Runtime)?;
        }
        for c in &class_defs {
            self.register_class(c)?;
        }

        // Python-like imports: the module's top-level statements run once on load.
        for item in &program.items {
            if let TopLevel::Stmt(stmt) = item {
                let globals = Rc::clone(&self.globals);
                match self.execute_stmt(stmt, globals) {
                    Ok(ExecResult::Return(_)) => break,
                    Ok(_) => {}
                    Err(e) => {
                        self.base_dir = saved_base;
                        self.module_loader.loading.pop();
                        return Err(InterpreterError::Runtime(e));
                    }
                }
            }
        }

        self.base_dir = saved_base;
        self.module_loader.loading.pop();

        let module = Rc::new(Module {
            path: key.clone(),
            exports,
            structs: module_structs,
            classes: module_classes,
            traits: module_traits,
        });
        self.module_loader.modules.insert(key, Rc::clone(&module));
        self.import_exports(&module);
        Ok(())
    }

    fn import_exports(&mut self, module: &Module) {
        for (name, val) in &module.exports {
            self.globals.define(name.clone(), Rc::clone(val));
        }
        for (name, s) in &module.structs {
            self.structs.insert(name.clone(), s.clone());
        }
        for (name, t) in &module.traits {
            let _ = self.class_registry.borrow_mut().register_trait(t);
            let _ = name;
        }
        for (name, c) in &module.classes {
            let _ = self.register_class(c);
            let _ = name;
        }
    }

    fn resolve_module_path(&self, import_path: &str) -> PathBuf {
        let path = import_path.trim_matches('"');
        if let Some(stdlib) = &self.stdlib_dir {
            if let Some(resolved) = resolve_stdlib_path(stdlib, path) {
                return resolved;
            }
        }
        if path.ends_with(".niao") {
            self.base_dir.join(path)
        } else {
            self.base_dir.join(format!("{path}.niao"))
        }
    }

    fn define_function(
        &mut self,
        def: &FnDef,
        closure: Rc<Environment>,
    ) -> Result<(), InterpreterError> {
        let func = Value::Function(FunctionValue {
            def: def.clone(),
            closure,
        });
        self.globals.define(def.name.clone(), func.ref_cell());
        Ok(())
    }

    fn call_value(
        &mut self,
        callee: ValueRef,
        args: &[ValueRef],
        span: Span,
    ) -> Result<ValueRef, RuntimeError> {
        match &*callee.borrow() {
            Value::Function(func) => self.call_function(func, args),
            Value::NativeFunction(native) => native(args, span),
            other => Err(RuntimeError::TypeError {
                message: format!("{} is not callable", other.type_name()),
                line: span.line,
                col: span.col,
            }),
        }
    }

    fn call_function(
        &mut self,
        func: &FunctionValue,
        args: &[ValueRef],
    ) -> Result<ValueRef, RuntimeError> {
        if func.def.params.len() != args.len() {
            return Err(RuntimeError::at(
                func.def.span,
                1004,
                format!(
                    "{}() expected {} arguments, got {}",
                    func.def.name,
                    func.def.params.len(),
                    args.len()
                ),
            ));
        }

        let env = Environment::child(Rc::clone(&func.closure));
        for (param, arg) in func.def.params.iter().zip(args.iter()) {
            env.define(param.name.clone(), Rc::clone(arg));
        }

        match self.execute_block(&func.def.body, env)? {
            ExecResult::Return(val) => Ok(val),
            ExecResult::Value(val) => Ok(val),
            ExecResult::Break | ExecResult::Continue => {
                Err(RuntimeError::at(func.def.span, 1005, "invalid control flow"))
            }
        }
    }

    fn execute_block(
        &mut self,
        block: &Block,
        env: Rc<Environment>,
    ) -> Result<ExecResult, RuntimeError> {
        let mut last = Value::Nil.ref_cell();
        for stmt in &block.stmts {
            match self.execute_stmt(stmt, Rc::clone(&env))? {
                ExecResult::Return(v) => return Ok(ExecResult::Return(v)),
                ExecResult::Break => return Ok(ExecResult::Break),
                ExecResult::Continue => return Ok(ExecResult::Continue),
                ExecResult::Value(v) => last = v,
            }
        }
        Ok(ExecResult::Value(last))
    }

    fn execute_stmt(
        &mut self,
        stmt: &Stmt,
        env: Rc<Environment>,
    ) -> Result<ExecResult, RuntimeError> {
        match stmt {
            Stmt::VarDecl { name, init, span: _, .. } => {
                let val = if let Some(expr) = init {
                    self.eval_expr(expr, Rc::clone(&env))?
                } else {
                    Value::Nil.ref_cell()
                };
                env.define(name.clone(), val);
                Ok(ExecResult::Value(Value::Nil.ref_cell()))
            }
            Stmt::Assign {
                target,
                op,
                value,
                span,
            } => {
                let val = self.eval_expr(value, Rc::clone(&env))?;
                match target {
                    AssignTarget::Name(name) => {
                        if *op == AssignOp::Assign {
                            if !env.assign(name, Rc::clone(&val)) {
                                return Err(RuntimeError::UndefinedVar {
                                    name: name.clone(),
                                    line: span.line,
                                    col: span.col,
                                });
                            }
                        } else {
                            let current = env.get(name).ok_or(RuntimeError::UndefinedVar {
                                name: name.clone(),
                                line: span.line,
                                col: span.col,
                            })?;
                            let new_val = match op {
                                AssignOp::AddAssign => apply_binop(
                                    BinOp::Add,
                                    &current.borrow(),
                                    &val.borrow(),
                                    *span,
                                )?,
                                AssignOp::SubAssign => apply_binop(
                                    BinOp::Sub,
                                    &current.borrow(),
                                    &val.borrow(),
                                    *span,
                                )?,
                                AssignOp::Assign => unreachable!(),
                            };
                            env.assign(name, new_val.ref_cell());
                        }
                    }
                    AssignTarget::Member { object, field } => {
                        let obj = self.eval_expr(object, Rc::clone(&env))?;
                        let mut obj_ref = obj.borrow_mut();
                        match &mut *obj_ref {
                            Value::Object(map) => {
                                map.insert(field.clone(), val);
                            }
                            Value::Instance(inst) => {
                                let class_name = inst.class_name.clone();
                                self.class_registry.borrow().check_field_access(
                                    &class_name,
                                    field,
                                    self.current_class_name(),
                                )?;
                                inst.fields.insert(field.clone(), val);
                            }
                            _ => {
                                return Err(RuntimeError::TypeError {
                                    message: "cannot assign to member of non-object".into(),
                                    line: span.line,
                                    col: span.col,
                                });
                            }
                        }
                    }
                    AssignTarget::Index { object, index } => {
                        let obj = self.eval_expr(object, Rc::clone(&env))?;
                        let idx = self.eval_expr(index, Rc::clone(&env))?;
                        let i = match &*idx.borrow() {
                            Value::Int(n) => *n as usize,
                            _ => {
                                return Err(RuntimeError::TypeError {
                                    message: "array index must be int".into(),
                                    line: span.line,
                                    col: span.col,
                                });
                            }
                        };
                        let mut obj_ref = obj.borrow_mut();
                        match &mut *obj_ref {
                            Value::IntArray(arr) => {
                                if i >= arr.len() {
                                    return Err(RuntimeError::at(
                                        *span,
                                        1006,
                                        format!("index {i} out of bounds"),
                                    ));
                                }
                                let Value::Int(v) = &*val.borrow() else {
                                    return Err(RuntimeError::TypeError {
                                        message: "int array index requires int value".into(),
                                        line: span.line,
                                        col: span.col,
                                    });
                                };
                                arr[i] = *v;
                            }
                            Value::ByteArray(arr) => {
                                if i >= arr.len() {
                                    return Err(RuntimeError::at(
                                        *span,
                                        1006,
                                        format!("index {i} out of bounds"),
                                    ));
                                }
                                let Value::Int(v) = &*val.borrow() else {
                                    return Err(RuntimeError::TypeError {
                                        message: "byte array index requires int value".into(),
                                        line: span.line,
                                        col: span.col,
                                    });
                                };
                                if !(0..=255).contains(v) {
                                    return Err(RuntimeError::TypeError {
                                        message: "byte array values must be 0..=255".into(),
                                        line: span.line,
                                        col: span.col,
                                    });
                                }
                                arr[i] = *v as u8;
                            }
                            Value::StringArray(arr) => {
                                if !arr.set(i, {
                                    let Value::String(s) = &*val.borrow() else {
                                        return Err(RuntimeError::TypeError {
                                            message: "string array index requires string value".into(),
                                            line: span.line,
                                            col: span.col,
                                        });
                                    };
                                    s.clone()
                                }) {
                                    return Err(RuntimeError::at(
                                        *span,
                                        1006,
                                        format!("index {i} out of bounds"),
                                    ));
                                }
                            }
                            Value::Array(arr) => {
                                if i >= arr.len() {
                                    return Err(RuntimeError::at(
                                        *span,
                                        1006,
                                        format!("index {i} out of bounds"),
                                    ));
                                }
                                arr[i] = val;
                            }
                            _ => {
                                return Err(RuntimeError::TypeError {
                                    message: "cannot index non-array".into(),
                                    line: span.line,
                                    col: span.col,
                                });
                            }
                        }
                    }
                }
                Ok(ExecResult::Value(Value::Nil.ref_cell()))
            }
            Stmt::Expr(expr) => {
                let val = self.eval_expr(expr, env)?;
                Ok(ExecResult::Value(val))
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let cond_val = self.eval_expr(cond, Rc::clone(&env))?;
                if cond_val.borrow().is_truthy() {
                    self.execute_block(then_block, env)
                } else if let Some(else_blk) = else_block {
                    self.execute_block(else_blk, env)
                } else {
                    Ok(ExecResult::Value(Value::Nil.ref_cell()))
                }
            }
            Stmt::While { cond, body, .. } => {
                if dsa_fuse::try_run_while_fused(cond, body, &env) {
                    return Ok(ExecResult::Value(Value::Nil.ref_cell()));
                }
                loop {
                    let cond_val = self.eval_expr(cond, Rc::clone(&env))?;
                    if !cond_val.borrow().is_truthy() {
                        break;
                    }
                    match self.execute_block(body, Rc::clone(&env))? {
                        ExecResult::Break => break,
                        ExecResult::Continue => continue,
                        ExecResult::Return(v) => return Ok(ExecResult::Return(v)),
                        ExecResult::Value(_) => {}
                    }
                }
                Ok(ExecResult::Value(Value::Nil.ref_cell()))
            }
            Stmt::For { var, iter, body, .. } => {
                let iterable = self.eval_expr(iter, Rc::clone(&env))?;
                let items = match &*iterable.borrow() {
                    Value::Array(a) => a.clone(),
                    _ => {
                        return Err(RuntimeError::TypeError {
                            message: "for loop requires array".into(),
                            line: body.span.line,
                            col: body.span.col,
                        });
                    }
                };
                for item in items {
                    let loop_env = Environment::child(Rc::clone(&env));
                    loop_env.define(var.clone(), item);
                    match self.execute_block(body, loop_env)? {
                        ExecResult::Break => break,
                        ExecResult::Continue => continue,
                        ExecResult::Return(v) => return Ok(ExecResult::Return(v)),
                        ExecResult::Value(_) => {}
                    }
                }
                Ok(ExecResult::Value(Value::Nil.ref_cell()))
            }
            Stmt::Return { value, .. } => {
                let val = if let Some(expr) = value {
                    self.eval_expr(expr, env)?
                } else {
                    Value::Nil.ref_cell()
                };
                Ok(ExecResult::Return(val))
            }
            Stmt::Try {
                try_block,
                catch_var,
                catch_block,
                ..
            } => {
                match self.execute_block(try_block, Rc::clone(&env)) {
                    Ok(result) => Ok(result),
                    Err(e) => {
                        let catch_env = Environment::child(Rc::clone(&env));
                        let catch_val = match e {
                            RuntimeError::Thrown(v) => Value::Error(v).ref_cell(),
                            other => error_from_runtime(&other),
                        };
                        catch_env.define(catch_var.clone(), catch_val);
                        self.execute_block(catch_block, catch_env)
                    }
                }
            }
            Stmt::Throw { value, span } => {
                let val = self.eval_expr(value, Rc::clone(&env))?;
                let err = match &*val.borrow() {
                    Value::Error(e) => RuntimeError::thrown(e.clone()),
                    other => RuntimeError::thrown(NiaoErrorValue::from_message(
                        other.to_string(),
                        *span,
                    )),
                };
                Err(err)
            }
            Stmt::Break(_) => Ok(ExecResult::Break),
            Stmt::Continue(_) => Ok(ExecResult::Continue),
        }
    }

    fn eval_expr(
        &mut self,
        expr: &Expr,
        env: Rc<Environment>,
    ) -> Result<ValueRef, RuntimeError> {
        match expr {
            Expr::Int(v, _) => Ok(Value::Int(*v).ref_cell()),
            Expr::Float(v, _) => Ok(Value::Float(*v).ref_cell()),
            Expr::String(v, _) => Ok(Value::String(v.clone()).ref_cell()),
            Expr::Bool(v, _) => Ok(Value::Bool(*v).ref_cell()),
            Expr::Nil(_) => Ok(Value::Nil.ref_cell()),
            Expr::Ident(name, span) => env.get(name).ok_or(RuntimeError::UndefinedVar {
                name: name.clone(),
                line: span.line,
                col: span.col,
            }),
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                let l = self.eval_expr(left, Rc::clone(&env))?;
                let r = self.eval_expr(right, env)?;
                let result = apply_binop(*op, &l.borrow(), &r.borrow(), *span)?;
                Ok(result.ref_cell())
            }
            Expr::Unary { op, expr, span } => {
                let val = self.eval_expr(expr, env)?;
                let result = apply_unaryop(*op, &val.borrow(), *span)?;
                Ok(result.ref_cell())
            }
            Expr::Call { callee, args, span } => {
                self.eval_call(callee, args, *span, env)
            }
            Expr::Member { object, field, span } => {
                self.eval_member(object, field, *span, env, false)
            }
            Expr::Index { object, index, span } => {
                let obj = self.eval_expr(object, Rc::clone(&env))?;
                let idx = self.eval_expr(index, env)?;
                let i = match &*idx.borrow() {
                    Value::Int(n) => *n as usize,
                    _ => {
                        return Err(RuntimeError::TypeError {
                            message: "array index must be int".into(),
                            line: span.line,
                            col: span.col,
                        });
                    }
                };
                let elem = {
                    let borrowed = obj.borrow();
                    match &*borrowed {
                        Value::IntArray(arr) => arr.get(i).copied().map(|n| Value::Int(n).ref_cell()),
                        Value::FloatArray(arr) => arr.get(i).copied().map(|n| Value::Float(n).ref_cell()),
                        Value::BoolArray(arr) => arr.get(i).copied().map(|n| Value::Bool(n != 0).ref_cell()),
                        Value::ByteArray(arr) => arr.get(i).copied().map(|n| Value::Int(n as i64).ref_cell()),
                        Value::StringArray(arr) => arr.get(i).map(|s| Value::String(s).ref_cell()),
                        Value::Array(arr) => arr.get(i).cloned(),
                        _ => None,
                    }
                };
                elem.ok_or(RuntimeError::at(
                    *span,
                    1008,
                    format!("index {i} out of bounds or invalid access"),
                ))
            }
            Expr::Array { elements, span: _ } => {
                let mut vals = Vec::new();
                for el in elements {
                    vals.push(self.eval_expr(el, Rc::clone(&env))?);
                }
                Ok(Value::Array(vals).ref_cell())
            }
            Expr::Object { fields, .. } => {
                let mut map = HashMap::new();
                for (name, expr) in fields {
                    map.insert(name.clone(), self.eval_expr(expr, Rc::clone(&env))?);
                }
                Ok(Value::Object(map).ref_cell())
            }
            Expr::StructInit { name, fields, span } => {
                if self.class_registry.borrow().get_class(name).is_some() {
                    return self.eval_class_init(name, fields, *span, env);
                }
                let struct_def = self.structs.get(name).cloned().ok_or(RuntimeError::at(
                    *span,
                    1009,
                    format!("unknown struct '{name}'"),
                ))?;
                let mut map = HashMap::new();
                for (fname, expr) in fields {
                    if !struct_def.fields.iter().any(|f| f.name == *fname) {
                        return Err(RuntimeError::at(
                            *span,
                            1010,
                            format!("unknown field '{fname}' in struct '{name}'"),
                        ));
                    }
                    map.insert(fname.clone(), self.eval_expr(expr, Rc::clone(&env))?);
                }
                Ok(Value::Object(map).ref_cell())
            }
            Expr::ClassInit { name, fields, span } => self.eval_class_init(name, fields, *span, env),
            Expr::SuperCall { method, args, span } => {
                self.eval_super_call(method, args, *span, env)
            }
        }
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
        env: Rc<Environment>,
    ) -> Result<ValueRef, RuntimeError> {
        if let Expr::Member { object, field, span: member_span } = callee {
            if let Expr::Ident(class_name, _) = &**object {
                if self.class_registry.borrow().get_class(class_name).is_some() {
                    let mut arg_vals = Vec::new();
                    for arg in args {
                        arg_vals.push(self.eval_expr(arg, Rc::clone(&env))?);
                    }
                    return self.call_static(class_name, field, &arg_vals, *member_span);
                }
            }
            let obj_val = self.eval_expr(object, Rc::clone(&env))?;
            let mut arg_vals = Vec::new();
            for arg in args {
                arg_vals.push(self.eval_expr(arg, Rc::clone(&env))?);
            }
            if matches!(&*obj_val.borrow(), Value::Object(_)) {
                let method =
                    self.eval_member(object, field, *member_span, Rc::clone(&env), true)?;
                return self.call_value(method, &arg_vals, span);
            }
            return self.call_method(obj_val, field, &arg_vals, span);
        }
        let func = self.eval_expr(callee, Rc::clone(&env))?;
        let mut arg_vals = Vec::new();
        for arg in args {
            arg_vals.push(self.eval_expr(arg, Rc::clone(&env))?);
        }
        self.call_value(func, &arg_vals, span)
    }

    fn eval_member(
        &mut self,
        object: &Expr,
        field: &str,
        span: Span,
        env: Rc<Environment>,
        for_call: bool,
    ) -> Result<ValueRef, RuntimeError> {
        if let Expr::Ident(class_name, _) = object {
            if let Some(class) = self.class_registry.borrow().get_class(class_name).cloned() {
                if let Some(method) = class.static_methods.get(field) {
                    return Ok(Value::Function(method.clone()).ref_cell());
                }
                if let Some(val) = class.static_fields.borrow().get(field) {
                    return Ok(Rc::clone(val));
                }
                if !for_call {
                    return Err(RuntimeError::at(
                        span,
                        1021,
                        format!("class '{class_name}' has no static member '{field}'"),
                    ));
                }
            }
        }
        let obj = self.eval_expr(object, env)?;
        let borrowed = obj.borrow();
        match &*borrowed {
            Value::Instance(inst) => {
                let class_name = inst.class_name.clone();
                let field_name = field.to_string();
                if let Some(class) = self.class_registry.borrow().get_class(&class_name).cloned() {
                    if class.methods.contains_key(&field_name) {
                        return Err(RuntimeError::at(
                            span,
                            1025,
                            format!("'{field_name}' is a method; call it with ()"),
                        ));
                    }
                    self.class_registry.borrow().check_field_access(
                        &class_name,
                        &field_name,
                        self.current_class_name(),
                    )?;
                    return inst
                        .fields
                        .get(&field_name)
                        .cloned()
                        .ok_or_else(|| {
                            RuntimeError::at(
                                span,
                                1021,
                                format!("field '{field_name}' not found on instance"),
                            )
                        });
                }
                Err(RuntimeError::at(
                    span,
                    1020,
                    format!("unknown class '{class_name}'"),
                ))
            }
            Value::Object(_) => Ok(borrowed
                .object_get_field(field)
                .ok_or(RuntimeError::TypeError {
                    message: format!("field '{field}' not found"),
                    line: span.line,
                    col: span.col,
                })?),
            Value::BsonDoc(_) => Ok(borrowed
                .object_get_field(field)
                .ok_or(RuntimeError::TypeError {
                    message: format!("field '{field}' not found"),
                    line: span.line,
                    col: span.col,
                })?),
            Value::Error(_) => error_field(&borrowed, field)
                .map(|v| v.ref_cell())
                .ok_or(RuntimeError::TypeError {
                    message: format!("field '{field}' not found"),
                    line: span.line,
                    col: span.col,
                }),
            _ => Err(RuntimeError::TypeError {
                message: "invalid member access".into(),
                line: span.line,
                col: span.col,
            }),
        }
    }

    fn call_method(
        &mut self,
        receiver: ValueRef,
        method: &str,
        args: &[ValueRef],
        span: Span,
    ) -> Result<ValueRef, RuntimeError> {
        let class_name = match &*receiver.borrow() {
            Value::Instance(inst) => inst.class_name.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    message: format!("cannot call method on {}", other.type_name()),
                    line: span.line,
                    col: span.col,
                });
            }
        };
        let method_entry = self
            .class_registry
            .borrow()
            .get_class(&class_name)
            .and_then(|c| c.methods.get(method).cloned())
            .ok_or_else(|| {
                RuntimeError::at(
                    span,
                    1021,
                    format!("method '{method}' not found on class '{class_name}'"),
                )
            })?;
        self.class_registry.borrow().check_method_access(
            &method_entry,
            self.current_class_name(),
        )?;
        let mut full_args = vec![Rc::clone(&receiver)];
        full_args.extend(args.iter().cloned());
        self.call_instance_method(&method_entry, &class_name, &full_args)
    }

    fn call_static(
        &mut self,
        class_name: &str,
        method: &str,
        args: &[ValueRef],
        span: Span,
    ) -> Result<ValueRef, RuntimeError> {
        let func = self
            .class_registry
            .borrow()
            .get_class(class_name)
            .and_then(|c| c.static_methods.get(method).cloned())
            .ok_or_else(|| {
                RuntimeError::at(
                    span,
                    1021,
                    format!("static method '{method}' not found on class '{class_name}'"),
                )
            })?;
        self.call_value(Value::Function(func).ref_cell(), args, span)
    }

    fn call_instance_method(
        &mut self,
        method: &InstanceMethod,
        class_name: &str,
        args: &[ValueRef],
    ) -> Result<ValueRef, RuntimeError> {
        let saved_class = self.current_class.clone();
        self.current_class = Some(class_name.to_string());
        push_method_context(MethodContext {
            class_name: class_name.to_string(),
        });
        let result = self.call_function(&method.func, args);
        pop_method_context();
        self.current_class = saved_class;
        result
    }

    fn eval_class_init(
        &mut self,
        name: &str,
        fields: &[(String, Expr)],
        span: Span,
        env: Rc<Environment>,
    ) -> Result<ValueRef, RuntimeError> {
        let class = self
            .class_registry
            .borrow()
            .get_class(name)
            .cloned()
            .ok_or_else(|| RuntimeError::at(span, 1020, format!("unknown class '{name}'")))?;
        let mut field_map = HashMap::new();
        for (fname, expr) in fields {
            if !class.fields.contains_key(fname) {
                return Err(RuntimeError::at(
                    span,
                    1010,
                    format!("unknown field '{fname}' in class '{name}'"),
                ));
            }
            field_map.insert(fname.clone(), self.eval_expr(expr, Rc::clone(&env))?);
        }
        Ok(Value::Instance(InstanceValue {
            class_name: name.to_string(),
            fields: field_map,
        })
        .ref_cell())
    }

    fn eval_super_call(
        &mut self,
        method: &str,
        args: &[Expr],
        span: Span,
        env: Rc<Environment>,
    ) -> Result<ValueRef, RuntimeError> {
        let ctx = current_method_context().ok_or_else(|| {
            RuntimeError::at(span, 1023, "super call outside of instance method")
        })?;
        let parent_method = self
            .class_registry
            .borrow()
            .resolve_super_method(&ctx.class_name, method)?;
        let self_val = env.get("self").ok_or_else(|| RuntimeError::at(
            span,
            1023,
            "super call requires self in scope",
        ))?;
        let mut arg_vals = vec![self_val];
        for arg in args {
            arg_vals.push(self.eval_expr(arg, Rc::clone(&env))?);
        }
        let parent_name = self
            .class_registry
            .borrow()
            .get_class(&ctx.class_name)
            .and_then(|c| c.parent.clone())
            .unwrap_or_default();
        self.call_instance_method(&parent_method, &parent_name, &arg_vals)
    }
}

fn resolve_stdlib_path(stdlib: &Path, import_path: &str) -> Option<PathBuf> {
    let rel = import_path
        .strip_prefix("std/")
        .unwrap_or(import_path);
    let direct = stdlib.join(format!("{rel}.niao"));
    if direct.is_file() {
        return Some(direct);
    }
    let lib_file = stdlib.join(rel).join("lib.niao");
    if lib_file.is_file() {
        return Some(lib_file);
    }
    None
}

pub fn run(source: &str) -> Result<ValueRef, InterpreterError> {
    Interpreter::new().run_source(source)
}

pub fn run_file(path: &Path) -> Result<ValueRef, InterpreterError> {
    Interpreter::new().run_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_hello() {
        let src = include_str!("../../../examples/hello.niao");
        run(src).unwrap();
    }

    #[test]
    fn runs_fibonacci() {
        let src = include_str!("../../../examples/fibonacci.niao");
        run(src).unwrap();
    }

    #[test]
    fn runs_super_booster_sort() {
        let src = include_str!("../../../examples/super_booster_sort.niao");
        run(src).unwrap();
    }
}
