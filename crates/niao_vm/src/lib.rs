mod dsa_fast;
mod dsa_loops;
mod fast_val;
mod gc;
mod io_fast;
mod json_fast;
mod ncl_fast;
#[cfg(feature = "nllm")]
mod nllm_fast;
mod nml_fast;
mod nmongo_fast;
#[cfg(feature = "nrag")]
mod nrag_fast;
mod turbo;
pub mod call_bridge;
pub mod ahiru_pool;

use dsa_loops::{DsaLoopRegion, DsaLoopState};
use fast_val::{value_to_fast, FastVal, HeapMut};
use gc::MemoCache;
use niao_ast::{BinOp, Span, UnaryOp};
use niao_bytecode::{BytecodeFunction, BytecodeModule, FastPath, OpCode};
use niao_runtime::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug)]
pub enum VmError {
    Runtime(RuntimeError),
    StackUnderflow,
    UnknownFunction(String),
    NoMain,
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::Runtime(e) => write!(f, "{e}"),
            VmError::StackUnderflow => write!(f, "stack underflow"),
            VmError::UnknownFunction(name) => write!(f, "unknown function: {name}"),
            VmError::NoMain => write!(f, "no main function found"),
        }
    }
}

impl std::error::Error for VmError {}

impl From<RuntimeError> for VmError {
    fn from(e: RuntimeError) -> Self {
        VmError::Runtime(e)
    }
}

pub struct Vm {
    stack: Vec<FastVal>,
    pub(crate) heap: Vec<ValueRef>,
    globals: Rc<Environment>,
    frames: Vec<Frame>,
    frame_pool: Vec<Vec<FastVal>>,
    functions: Vec<BytecodeFunction>,
    constants: Vec<FastVal>,
    native_indices: HashMap<u16, ValueRef>,
    user_fn_indices: HashSet<u16>,
    print_native_idx: Option<u16>,
    print_super_boom_factorial_native_idx: Option<u16>,
    super_boom_factorial_native_idx: Option<u16>,
    print_super_boom_math_native_idx: Option<u16>,
    super_boom_math_native_idx: Option<u16>,
    memo_caches: Vec<Option<MemoCache>>,
    alloc_since_gc: u32,
    gc_threshold: usize,
    /// Per function: (backward-jump ip, compiled turbo loop region).
    loop_regions: Vec<Vec<(usize, Rc<turbo::LoopRegion>)>>,
    /// Hot DSA builtins with unboxed int fast paths.
    dsa_fast_paths: HashMap<u16, dsa_fast::DsaFastPath>,
    nml_fast_paths: HashMap<u16, nml_fast::NmlFastPath>,
    ncl_fast_paths: HashMap<u16, ncl_fast::NclFastPath>,
    json_fast_paths: HashMap<u16, json_fast::JsonFastPath>,
    io_fast_paths: HashMap<u16, io_fast::IoFastPath>,
    nmongo_fast_paths: HashMap<u16, nmongo_fast::NmongoFastPath>,
    #[cfg(feature = "nrag")]
    nrag_fast_paths: HashMap<u16, nrag_fast::NragFastPath>,
    #[cfg(feature = "nllm")]
    nllm_fast_paths: HashMap<u16, nllm_fast::NllmFastPath>,
    /// Native DSA handles (indexed by `FastVal::Native`).
    native_ds: Vec<Rc<RefCell<NativeDs>>>,
    native_refs: Arc<Vec<ValueRef>>,
    /// Fused DSA loops keyed by loop-head bytecode ip.
    dsa_loops: Vec<HashMap<usize, DsaLoopRegion>>,
    dsa_loop_state: Vec<DsaLoopState>,
    field_names: Vec<String>,
    slot_names: Vec<String>,
    classes: Vec<niao_ast::ClassDef>,
    call_target_names: Vec<String>,
    class_registry: Option<Rc<RefCell<ClassRegistry>>>,
    try_stack: Vec<TryHandler>,
    call_args: Vec<ValueRef>,
    gc_defer: u32,
    /// (class_name_hash << 16 | field_idx) -> user function index (C6).
    method_cache: HashMap<u64, usize>,
}

struct Frame {
    func_idx: usize,
    ip: usize,
    locals: Vec<FastVal>,
    memo_key: Option<i64>,
}

#[derive(Clone, Copy)]
struct TryHandler {
    catch_ip: usize,
    frame_depth: usize,
    stack_len: usize,
}

enum StepOutcome {
    Continue,
    Done,
}

impl Vm {
    pub fn new() -> Self {
        let globals = builtin_environment();
        Self {
            stack: Vec::with_capacity(4096),
            heap: Vec::with_capacity(64),
            globals,
            frames: Vec::with_capacity(128),
            frame_pool: Vec::new(),
            functions: Vec::new(),
            constants: Vec::new(),
            native_indices: HashMap::new(),
            user_fn_indices: HashSet::new(),
            print_native_idx: None,
            print_super_boom_factorial_native_idx: None,
            super_boom_factorial_native_idx: None,
            print_super_boom_math_native_idx: None,
            super_boom_math_native_idx: None,
            memo_caches: Vec::new(),
            alloc_since_gc: 0,
            gc_threshold: gc::GC_THRESHOLD_INITIAL,
            loop_regions: Vec::new(),
            dsa_fast_paths: HashMap::new(),
            nml_fast_paths: HashMap::new(),
            ncl_fast_paths: HashMap::new(),
            json_fast_paths: HashMap::new(),
            io_fast_paths: HashMap::new(),
            nmongo_fast_paths: HashMap::new(),
            #[cfg(feature = "nrag")]
            nrag_fast_paths: HashMap::new(),
            #[cfg(feature = "nllm")]
            nllm_fast_paths: HashMap::new(),
            native_ds: Vec::new(),
            native_refs: Arc::new(Vec::new()),
            dsa_loops: Vec::new(),
            dsa_loop_state: Vec::new(),
            field_names: Vec::new(),
            slot_names: Vec::new(),
            classes: Vec::new(),
            call_target_names: Vec::new(),
            class_registry: None,
            try_stack: Vec::new(),
            call_args: Vec::with_capacity(8),
            gc_defer: 0,
            method_cache: HashMap::new(),
        }
    }

    /// Load bytecode definitions without executing the entry function.
    pub fn init_module(&mut self, module: &BytecodeModule, _base_dir: &Path) -> Result<(), VmError> {
        if module.fast_path.is_some() {
            return Err(VmError::Runtime(RuntimeError::at(
                Span::dummy(),
                niao_errors::codes::E2003_TYPE_ERROR,
                "fast-path modules cannot be used as ahiru handler VMs",
            )));
        }
        self.load_module(module);
        Ok(())
    }

    pub fn arena_sizes(&self) -> (usize, usize) {
        (self.heap.len(), self.native_ds.len())
    }

    pub fn run(&mut self, module: &BytecodeModule, base_dir: &Path) -> Result<(), VmError> {
        if let Some(path) = module.fast_path {
            run_fast_path(path);
            return Ok(());
        }
        let vm_ptr = self as *mut Vm;
        niao_runtime::mem::set_vm_arena_reader(Some(Rc::new(move || {
            // SAFETY: reader is only invoked on the VM thread while `run` is active.
            unsafe { (*vm_ptr).arena_sizes() }
        })));
        let result = self.run_loaded(module, base_dir);
        niao_runtime::mem::set_vm_arena_reader(None);
        result
    }

    fn run_loaded(&mut self, module: &BytecodeModule, _base_dir: &Path) -> Result<(), VmError> {
        self.load_module(module);
        // Entry point: synthetic top-level script function if the program has
        // top-level statements (it calls main itself), otherwise main directly.
        let entry_idx = self
            .functions
            .iter()
            .position(|f| f.name == niao_ir::TOPLEVEL_FN)
            .or_else(|| self.functions.iter().position(|f| f.name == "main"));
        let Some(entry_idx) = entry_idx else {
            // Nothing to execute (only definitions): Python-like no-op.
            return Ok(());
        };
        self.enter_frame(entry_idx, 0)?;
        self.dispatch()?;
        flush_print_buffer();
        Ok(())
    }

    fn handle_try(&mut self, err: &VmError) -> Result<bool, VmError> {
        let VmError::Runtime(runtime_err) = err else {
            return Ok(false);
        };
        let Some(handler) = self.try_stack.pop() else {
            return Ok(false);
        };
        while self.frames.len() > handler.frame_depth {
            let locals = self.frames.pop().unwrap().locals;
            self.frame_pool.push(locals);
        }
        self.stack.truncate(handler.stack_len);
        let idx = self.alloc_heap(error_from_runtime(runtime_err));
        self.stack.push(FastVal::Heap(idx));
        if let Some(frame) = self.frames.last_mut() {
            frame.ip = handler.catch_ip;
        }
        Ok(true)
    }

    fn load_module(&mut self, module: &BytecodeModule) {
        self.heap.clear();
        self.native_ds.clear();
        self.native_refs = Arc::new(Vec::new());
        self.method_cache.clear();
        self.reset_gc_state();
        self.constants = {
            let mut heap = HeapMut { vm: self };
            module
                .constants
                .iter()
                .map(|c| FastVal::from_const(c, &mut heap))
                .collect()
        };

        let user_fn_count = module.functions.len();
        self.native_indices.clear();
        self.user_fn_indices.clear();
        self.dsa_fast_paths.clear();
        self.nml_fast_paths.clear();
        self.nmongo_fast_paths.clear();
        #[cfg(feature = "nrag")]
        self.nrag_fast_paths.clear();
        #[cfg(feature = "nllm")]
        self.nllm_fast_paths.clear();
        self.dsa_loops.clear();
        self.dsa_loop_state.clear();
        self.try_stack.clear();
        self.print_native_idx = module
            .call_targets
            .iter()
            .position(|name| name == "print")
            .map(|i| i as u16);
        self.print_super_boom_factorial_native_idx = module
            .call_targets
            .iter()
            .position(|name| name == "print_super_boom_factorial")
            .map(|i| i as u16);
        self.super_boom_factorial_native_idx = module
            .call_targets
            .iter()
            .position(|name| name == "super_boom_factorial")
            .map(|i| i as u16);
        self.print_super_boom_math_native_idx = module
            .call_targets
            .iter()
            .position(|name| name == "print_super_boom_math")
            .map(|i| i as u16);
        self.super_boom_math_native_idx = module
            .call_targets
            .iter()
            .position(|name| name == "super_boom_math")
            .map(|i| i as u16);

        for (i, name) in module.call_targets.iter().enumerate() {
            let idx = i as u16;
            if i < user_fn_count {
                self.user_fn_indices.insert(idx);
            }
            if let Some(path) = dsa_fast::DsaFastPath::from_name(name) {
                self.dsa_fast_paths.insert(idx, path);
            }
            if let Some(path) = ncl_fast::NclFastPath::from_name(name) {
                self.ncl_fast_paths.insert(idx, path);
            }
            if let Some(path) = json_fast::JsonFastPath::from_name(name) {
                self.json_fast_paths.insert(idx, path);
            }
            if let Some(path) = io_fast::IoFastPath::from_name(name) {
                self.io_fast_paths.insert(idx, path);
            }
            if let Some(path) = nml_fast::NmlFastPath::from_name(name) {
                self.nml_fast_paths.insert(idx, path);
            }
            if let Some(path) = nmongo_fast::NmongoFastPath::from_name(name) {
                self.nmongo_fast_paths.insert(idx, path);
            }
            #[cfg(feature = "nrag")]
            if let Some(path) = nrag_fast::NragFastPath::from_name(name) {
                self.nrag_fast_paths.insert(idx, path);
            }
            #[cfg(feature = "nllm")]
            if let Some(path) = nllm_fast::NllmFastPath::from_name(name) {
                self.nllm_fast_paths.insert(idx, path);
            }
            if let Some(val) = self.globals.get(name) {
                if matches!(&*val.borrow(), Value::NativeFunction(_)) {
                    self.native_indices.insert(idx, val);
                }
            }
        }

        self.functions = module.functions.clone();
        self.field_names = module.field_names.clone();
        self.classes = module.classes.clone();
        self.call_target_names = module.call_targets.clone();

        let mut registry = ClassRegistry::new();
        registry.register_metadata(&module.traits, &module.classes);
        let registry = Rc::new(RefCell::new(registry));
        set_class_registry(Rc::clone(&registry));
        self.class_registry = Some(registry);

        self.memo_caches = self
            .functions
            .iter()
            .map(|f| if f.memoize { Some(MemoCache::new()) } else { None })
            .collect();
        self.loop_regions = self
            .functions
            .iter()
            .map(|f| turbo::find_regions(&f.code, &self.constants))
            .collect();

        let fast_by_fidx: HashMap<u16, u8> = self
            .dsa_fast_paths
            .iter()
            .map(|(&k, v)| (k, v.0))
            .collect();
        self.dsa_loops = self
            .functions
            .iter()
            .map(|f| dsa_loops::scan_loops(&f.code, &module.constants, &fast_by_fidx))
            .collect();
        self.dsa_loop_state = (0..self.functions.len())
            .map(|_| DsaLoopState::default())
            .collect();
        self.field_names = module.field_names.clone();
        self.slot_names = module.slot_names.clone();
    }

    fn fast_to_value_ref(&self, v: FastVal) -> ValueRef {
        match v {
            FastVal::Native(i) => Rc::clone(&self.native_refs[i as usize]),
            other => other.to_value_ref(&self.heap, &self.native_refs),
        }
    }

    fn alloc_locals(&mut self, count: usize) -> Vec<FastVal> {
        if let Some(mut locals) = self.frame_pool.pop() {
            if locals.len() < count {
                locals.resize(count, FastVal::NIL);
            } else {
                locals.truncate(count);
                locals.fill(FastVal::NIL);
            }
            locals
        } else {
            vec![FastVal::NIL; count]
        }
    }

    fn enter_frame(&mut self, func_idx: usize, argc: usize) -> Result<(), VmError> {
        let local_count = self.functions[func_idx].local_count as usize;
        let memoize = self.functions[func_idx].memoize;
        let param_slots = self.functions[func_idx].param_slots.clone();

        // Memoized hit: O(1) return for self-recursive functions like fib
        if memoize && argc == 1 {
            if let Some(&arg) = self.stack.last() {
                if let FastVal::Int(key) = arg {
                    if let Some(cache) = &self.memo_caches[func_idx] {
                        if let Some(&result) = cache.get(&key) {
                            self.stack.pop();
                            self.stack.push(result);
                            return Ok(());
                        }
                    }
                }
            }
        }

        let mut locals = self.alloc_locals(local_count);
        let mut memo_key = None;

        let param_count = param_slots.len().min(argc);
        for i in (0..param_count).rev() {
            let arg = self.stack.pop().ok_or(VmError::StackUnderflow)?;
            if memoize && i == 0 {
                if let FastVal::Int(key) = arg {
                    memo_key = Some(key);
                }
            }
            let slot = param_slots[i] as usize;
            if slot < local_count {
                locals[slot] = arg;
            }
        }

        self.frames.push(Frame {
            func_idx,
            ip: 0,
            locals,
            memo_key,
        });
        self.dsa_loop_state[func_idx].reset();
        Ok(())
    }

    #[inline(never)]
    fn dispatch(&mut self) -> Result<(), VmError> {
        loop {
            match self.dispatch_step() {
                Ok(StepOutcome::Done) => return Ok(()),
                Ok(StepOutcome::Continue) => {}
                Err(e) => {
                    if self.handle_try(&e)? {
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    #[inline(never)]
    fn dispatch_step(&mut self) -> Result<StepOutcome, VmError> {
        self.maybe_collect();

        let frame_top = match self.frames.len() {
            0 => return Ok(StepOutcome::Done),
            n => n - 1,
        };

        let (func_idx, ip, code_len) = {
            let frame = &self.frames[frame_top];
            let len = self.functions[frame.func_idx].code.len();
            (frame.func_idx, frame.ip, len)
        };

        if ip >= code_len {
            let locals = self.frames.pop().unwrap().locals;
            self.frame_pool.push(locals);
            return Ok(StepOutcome::Continue);
        }

        if let Some(region) = self.dsa_loops[func_idx].get(&ip) {
            if self.dsa_loop_state[func_idx].may_fuse(ip) {
                if let Some(exit) = dsa_loops::run_fused(
                    region,
                    &mut self.frames[frame_top].locals,
                    &self.native_ds,
                    &self.heap,
                ) {
                    self.dsa_loop_state[func_idx].mark_fused(ip);
                    self.frames[frame_top].ip = exit;
                    return Ok(StepOutcome::Continue);
                }
            }
        }

        let op = &self.functions[func_idx].code[ip];
        self.frames[frame_top].ip = ip + 1;

        match op {
                OpCode::Const(idx) => {
                    self.stack.push(self.constants[*idx as usize]);
                }
                OpCode::Load(idx) => {
                    let slot = *idx as usize;
                    let mut val = self.frames[frame_top]
                        .locals
                        .get(slot)
                        .copied()
                        .unwrap_or(FastVal::NIL);
                    if matches!(val, FastVal::Nil) {
                        if let Some(name) = self.slot_names.get(slot).filter(|n| !n.is_empty()) {
                            if let Some(global) = self.globals.get(name) {
                                let global_ref = Rc::clone(&global);
                                val = if let Some(heap_idx) = self
                                    .heap
                                    .iter()
                                    .position(|cell| Rc::ptr_eq(cell, &global_ref))
                                {
                                    FastVal::Heap(heap_idx as u32)
                                } else {
                                    let mut heap = HeapMut { vm: self };
                                    value_to_fast(&global_ref.borrow(), &mut heap)
                                };
                                if slot < self.frames[frame_top].locals.len() {
                                    self.frames[frame_top].locals[slot] = val;
                                }
                            }
                        }
                    }
                    self.stack.push(val);
                }
                OpCode::Store(idx) => {
                    let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let slot = *idx as usize;
                    if slot < self.frames[frame_top].locals.len() {
                        self.frames[frame_top].locals[slot] = val;
                    }
                }
                OpCode::BindGlobal(idx) => {
                    let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let slot = *idx as usize;
                    if let Some(name) = self.slot_names.get(slot).filter(|n| !n.is_empty()) {
                        let value_ref = self.fast_to_value_ref(val);
                        self.globals.define(name.clone(), value_ref);
                    }
                    if slot < self.frames[frame_top].locals.len() {
                        self.frames[frame_top].locals[slot] = val;
                    }
                }
                OpCode::Add => self.do_int_binop(BinOp::Add)?,
                OpCode::Sub => self.do_int_binop(BinOp::Sub)?,
                OpCode::Mul => self.do_int_binop(BinOp::Mul)?,
                OpCode::Div => self.do_int_binop(BinOp::Div)?,
                OpCode::FloorDiv => self.do_int_binop(BinOp::FloorDiv)?,
                OpCode::Mod => self.do_int_binop(BinOp::Mod)?,
                OpCode::Eq => self.do_binop(BinOp::Eq)?,
                OpCode::Ne => self.do_binop(BinOp::Ne)?,
                OpCode::Lt => self.do_binop(BinOp::Lt)?,
                OpCode::Gt => self.do_binop(BinOp::Gt)?,
                OpCode::Le => self.do_binop(BinOp::Le)?,
                OpCode::Ge => self.do_binop(BinOp::Ge)?,
                OpCode::And => self.do_binop(BinOp::And)?,
                OpCode::Or => self.do_binop(BinOp::Or)?,
                OpCode::Not => self.do_unaryop(UnaryOp::Not)?,
                OpCode::Neg => self.do_unaryop(UnaryOp::Neg)?,
                OpCode::Call { func: fidx, argc } => {
                    let fidx = *fidx;
                    let argc = *argc as usize;
                    if self.print_super_boom_factorial_native_idx == Some(fidx) && argc == 1 {
                        if let Some(FastVal::Int(n)) = self.stack.last().copied() {
                            self.stack.pop();
                            print_super_boom_factorial_int(n);
                            return Ok(StepOutcome::Continue);
                        }
                    }
                    if self.super_boom_factorial_native_idx == Some(fidx) && argc == 1 {
                        if let Some(FastVal::Int(n)) = self.stack.pop() {
                            match super_boom_factorial_compute(n) {
                                Value::Int(v) => self.stack.push(FastVal::Int(v)),
                                Value::BigInt(b) => {
                                    let idx = self.alloc_heap(Value::BigInt(b).ref_cell());
                                    self.stack.push(FastVal::Heap(idx));
                                }
                                _ => self.stack.push(FastVal::NIL),
                            }
                            return Ok(StepOutcome::Continue);
                        }
                    }
                    if self.print_super_boom_math_native_idx == Some(fidx) && argc == 1 {
                        if let Some(FastVal::Int(n)) = self.stack.last().copied() {
                            self.stack.pop();
                            print_super_boom_math_int(n);
                            return Ok(StepOutcome::Continue);
                        }
                    }
                    if self.super_boom_math_native_idx == Some(fidx) && argc == 1 {
                        if let Some(FastVal::Int(n)) = self.stack.pop() {
                            self.stack.push(FastVal::Int(super_boom_math_compute(n)));
                            return Ok(StepOutcome::Continue);
                        }
                    }
                    if self.print_native_idx == Some(fidx) && argc == 1 {
                        if let Some(v) = self.stack.last().copied() {
                            match v {
                                FastVal::Int(n) => {
                                    self.stack.pop();
                                    print_int_line(n);
                                    return Ok(StepOutcome::Continue);
                                }
                                FastVal::Heap(idx) => {
                                    if let Value::BigInt(n) = &*self.heap[idx as usize].borrow() {
                                        self.stack.pop();
                                        print_bigint_line(n);
                                        return Ok(StepOutcome::Continue);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    let ncl_fast = self.ncl_fast_paths.get(&fidx).copied();
                    let json_fast = self.json_fast_paths.get(&fidx).copied();
                    let io_fast = self.io_fast_paths.get(&fidx).copied();
                    let nml_fast = self.nml_fast_paths.get(&fidx).copied();
                    let nmongo_fast = self.nmongo_fast_paths.get(&fidx).copied();
                    #[cfg(feature = "nrag")]
                    let nrag_fast = self.nrag_fast_paths.get(&fidx).copied();
                    #[cfg(feature = "nllm")]
                    let nllm_fast = self.nllm_fast_paths.get(&fidx).copied();
                    let dsa_fast = self.dsa_fast_paths.get(&fidx).copied();
                    let native = self.native_indices.get(&fidx).map(Rc::clone);

                    if let Some(path) = ncl_fast {
                        if ncl_fast::NclFastPath::try_execute(&mut self.stack, &self.heap, argc, path)
                        {
                            return Ok(StepOutcome::Continue);
                        }
                    }
                    if let Some(path) = json_fast {
                        let base = self.stack.len() - argc;
                        let arg_vals: Vec<FastVal> = self.stack[base..].to_vec();
                        self.stack.truncate(base);
                        let refs = Arc::clone(&self.native_refs);
                        let heap_snap = self.heap.clone();
                        let out = {
                            let mut heap = HeapMut { vm: self };
                            json_fast::JsonFastPath::try_execute(
                                &arg_vals,
                                &heap_snap,
                                &refs,
                                path,
                                &mut heap,
                            )
                        };
                        if let Some(v) = out {
                            self.stack.push(v);
                            return Ok(StepOutcome::Continue);
                        }
                        for v in arg_vals {
                            self.stack.push(v);
                        }
                    }
                    if let Some(path) = io_fast {
                        let base = self.stack.len() - argc;
                        let arg_vals: Vec<FastVal> = self.stack[base..].to_vec();
                        self.stack.truncate(base);
                        let refs = Arc::clone(&self.native_refs);
                        let heap_snap = self.heap.clone();
                        let out = {
                            let mut heap = HeapMut { vm: self };
                            io_fast::IoFastPath::try_execute(
                                &arg_vals,
                                &heap_snap,
                                &refs,
                                path,
                                &mut heap,
                            )
                        };
                        if let Some(v) = out {
                            self.stack.push(v);
                            return Ok(StepOutcome::Continue);
                        }
                        for v in arg_vals {
                            self.stack.push(v);
                        }
                    }
                    if let Some(path) = nml_fast {
                        let base = self.stack.len() - argc;
                        if matches!(path, nml_fast::NmlFastPath::BackwardStep)
                            && nml_fast::NmlFastPath::try_backward_step(&self.stack, &self.heap, argc)
                        {
                            self.stack.truncate(base);
                            self.stack.push(FastVal::Nil);
                            return Ok(StepOutcome::Continue);
                        }
                        if let Some(handle_id) = path.try_execute(&self.stack, &self.heap, argc) {
                            self.stack.truncate(base);
                            let idx = self.alloc_heap(Value::NmlHandle(handle_id).ref_cell());
                            self.stack.push(FastVal::Heap(idx));
                            return Ok(StepOutcome::Continue);
                        }
                    }
                    if let Some(path) = nmongo_fast {
                        let base = self.stack.len() - argc;
                        let args = nmongo_fast::args_from_stack(
                            &self.stack,
                            base,
                            argc,
                            &self.heap,
                            &self.native_refs,
                        );
                        if let Some(result) = nmongo_fast::NmongoFastPath::try_execute_args(&args, path) {
                            let out = {
                                let mut heap = fast_val::HeapMut { vm: self };
                                nmongo_fast::to_fast_val(result, &mut heap)
                            };
                            self.stack.truncate(base);
                            self.stack.push(out);
                            return Ok(StepOutcome::Continue);
                        }
                    }
                    #[cfg(feature = "nrag")]
                    if let Some(path) = nrag_fast {
                        if nrag_fast::NragFastPath::try_execute_stack(&mut self.stack, argc, path) {
                            return Ok(StepOutcome::Continue);
                        }
                        let base = self.stack.len() - argc;
                        let arg_vals: Vec<FastVal> = self.stack[base..].to_vec();
                        self.stack.truncate(base);
                        let refs = Arc::clone(&self.native_refs);
                        let heap_snap = self.heap.clone();
                        let out = {
                            let mut heap = HeapMut { vm: self };
                            nrag_fast::NragFastPath::try_execute_heap(
                                &arg_vals,
                                &heap_snap,
                                &refs,
                                path,
                                &mut heap,
                            )
                        };
                        if let Some(v) = out {
                            self.stack.push(v);
                            return Ok(StepOutcome::Continue);
                        }
                        for v in arg_vals {
                            self.stack.push(v);
                        }
                    }
                    #[cfg(feature = "nllm")]
                    if let Some(path) = nllm_fast {
                        if nllm_fast::NllmFastPath::try_execute_stack(
                            &mut self.stack,
                            &self.heap,
                            &self.native_refs,
                            argc,
                            path,
                        ) {
                            return Ok(StepOutcome::Continue);
                        }
                        let base = self.stack.len() - argc;
                        let arg_vals: Vec<FastVal> = self.stack[base..].to_vec();
                        self.stack.truncate(base);
                        let refs = Arc::clone(&self.native_refs);
                        let heap_snap = self.heap.clone();
                        let out = {
                            let mut heap = HeapMut { vm: self };
                            nllm_fast::NllmFastPath::try_execute_heap(
                                &arg_vals,
                                &heap_snap,
                                &refs,
                                path,
                                &mut heap,
                            )
                        };
                        if let Some(v) = out {
                            self.stack.push(v);
                            return Ok(StepOutcome::Continue);
                        }
                        for v in arg_vals {
                            self.stack.push(v);
                        }
                    }
                    if let Some(native) = native {
                        if let Some(path) = dsa_fast {
                            if let Some(result) =
                                path.execute(&mut self.stack, &self.heap, &self.native_ds, argc)
                            {
                                self.stack.push(result);
                                return Ok(StepOutcome::Continue);
                            }
                        }
                        self.call_args.clear();
                        self.call_args.reserve(argc);
                        for _ in 0..argc {
                            let v = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                            self.call_args.push(self.fast_to_value_ref(v));
                        }
                        self.call_args.reverse();
                        let result = self.call_native(&native, &self.call_args)?;
                        let out = match &*result.borrow() {
                            Value::Int(v) => FastVal::Int(*v),
                            Value::Float(v) => FastVal::Float(*v),
                            Value::Bool(v) => FastVal::Bool(*v),
                            Value::Nil => FastVal::Nil,
                            Value::Native(ds) => self.alloc_native(Rc::clone(ds)),
                            _ => FastVal::Heap(self.alloc_heap(Rc::clone(&result))),
                        };
                        self.stack.push(out);
                    } else if self.user_fn_indices.contains(&fidx) {
                        self.enter_frame(fidx as usize, argc)?;
                    } else {
                        return Err(VmError::UnknownFunction(format!("idx_{fidx}")));
                    }
                }
                OpCode::Return => {
                    let val = self.stack.pop().unwrap_or(FastVal::NIL);
                    let frame = self.frames.pop().unwrap();
                    if let Some(key) = frame.memo_key {
                        if let Some(cache) = &mut self.memo_caches[frame.func_idx] {
                            cache.insert(key, val);
                        }
                    }
                    self.frame_pool.push(frame.locals);
                    if self.frames.is_empty() {
                        return Ok(StepOutcome::Done);
                    }
                    self.stack.push(val);
                }
                OpCode::Jump(target) => {
                    let t = *target as usize;
                    if t <= ip {
                        // Loop backedge: hand remaining iterations to the turbo tier.
                        let region = self.loop_regions[func_idx]
                            .iter()
                            .find(|(j, _)| *j == ip)
                            .map(|(_, r)| Rc::clone(r));
                        if let Some(region) = region {
                            if let Some(resume) = self.run_turbo(&region, frame_top) {
                                self.frames[frame_top].ip = resume;
                                return Ok(StepOutcome::Continue);
                            }
                        }
                    }
                    self.frames[frame_top].ip = t;
                }
                OpCode::JumpIfFalse(target) => {
                    let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    if !val.is_truthy() {
                        self.frames[frame_top].ip = *target as usize;
                    }
                }
                OpCode::Pop => {
                    self.stack.pop();
                }
                OpCode::MakeArray(n) => {
                    let n = *n as usize;
                    let refs = &self.native_refs;
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n {
                        items.push(
                            self.stack
                                .pop()
                                .ok_or(VmError::StackUnderflow)?
                                .to_value_ref(&self.heap, &refs),
                        );
                    }
                    items.reverse();
                    let idx = self.alloc_heap(Value::Array(items).ref_cell());
                    self.stack.push(FastVal::Heap(idx));
                }
                OpCode::MakeObject(fields) => {
                    let f = fields.clone();
                    self.do_make_object(&f)?;
                }
                OpCode::MakeInstance { class, fields } => {
                    self.do_make_instance(*class, *fields)?
                }
                OpCode::GetField(idx) => self.do_get_field(*idx)?,
                OpCode::SetField(idx) => self.do_set_field(*idx)?,
                OpCode::GetIndex => self.do_get_index()?,
                OpCode::SetIndex => self.do_set_index()?,
                OpCode::CallMethod { field, argc } => {
                    self.do_call_method(*field, *argc as usize)?
                }
                OpCode::CallSuper { method, argc } => {
                    self.do_call_super(*method, *argc as usize)?
                }
                OpCode::TryBegin(catch_ip) => {
                    self.try_stack.push(TryHandler {
                        catch_ip: *catch_ip as usize,
                        frame_depth: self.frames.len(),
                        stack_len: self.stack.len(),
                    });
                }
                OpCode::TryEnd(end_ip) => {
                    self.try_stack.pop();
                    self.frames[frame_top].ip = *end_ip as usize;
                }
                OpCode::Throw => {
                    let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let value_ref = self.fast_to_value_ref(val);
                    let runtime_err = match &*value_ref.borrow() {
                        Value::Error(e) => RuntimeError::thrown(e.clone()),
                        other => RuntimeError::thrown(niao_errors::NiaoErrorValue::from_message(
                            other.to_string(),
                            Span::dummy(),
                        )),
                    };
                    return Err(VmError::Runtime(runtime_err));
                }
                OpCode::Halt => {
                    let locals = self.frames.pop().unwrap().locals;
                    self.frame_pool.push(locals);
                    if self.frames.is_empty() {
                        return Ok(StepOutcome::Done);
                    }
                }
            }
        Ok(StepOutcome::Continue)
    }

    /// Run a compiled loop region. Returns the bytecode ip to resume at, or
    /// None when the region doesn't apply (non-int locals).
    fn run_turbo(&mut self, region: &turbo::LoopRegion, frame_top: usize) -> Option<usize> {
        let mut regs = region.enter(&self.frames[frame_top].locals)?;
        self.gc_defer += 1;
        let resume = region.execute(&mut regs);
        self.gc_defer = self.gc_defer.saturating_sub(1);
        region.write_back(&regs, &mut self.frames[frame_top].locals);
        Some(resume)
    }

    #[inline(always)]
    fn do_int_binop(&mut self, op: BinOp) -> Result<(), VmError> {
        let rhs = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let lhs = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let out = if matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::FloorDiv | BinOp::Mod
        ) {
            match (lhs, rhs) {
                (FastVal::Int(a), FastVal::Int(b)) => {
                    match op {
                        BinOp::Add => match a.checked_add(b) {
                            Some(v) => FastVal::Int(v),
                            None => self.promote_bigint_binop(a, b, |x, y| x + y)?,
                        },
                        BinOp::Sub => match a.checked_sub(b) {
                            Some(v) => FastVal::Int(v),
                            None => self.promote_bigint_binop(a, b, |x, y| x - y)?,
                        },
                        BinOp::Mul => match a.checked_mul(b) {
                            Some(v) => FastVal::Int(v),
                            None => self.promote_bigint_binop(a, b, |x, y| x * y)?,
                        },
                        BinOp::Div => {
                            if b == 0 {
                                return Err(VmError::Runtime(RuntimeError::DivisionByZero {
                                    line: 0,
                                    col: 0,
                                }));
                            }
                            FastVal::Float(a as f64 / b as f64)
                        }
                        BinOp::FloorDiv => {
                            if b == 0 {
                                return Err(VmError::Runtime(RuntimeError::DivisionByZero {
                                    line: 0,
                                    col: 0,
                                }));
                            }
                            FastVal::Int(a / b)
                        }
                        BinOp::Mod => {
                            if b == 0 {
                                return Err(VmError::Runtime(RuntimeError::DivisionByZero {
                                    line: 0,
                                    col: 0,
                                }));
                            }
                            FastVal::Int(a % b)
                        }
                        _ => unreachable!(),
                    }
                }
                (FastVal::Float(a), FastVal::Float(b)) => match op {
                    BinOp::Add => FastVal::Float(a + b),
                    BinOp::Sub => FastVal::Float(a - b),
                    BinOp::Mul => FastVal::Float(a * b),
                    BinOp::Div => {
                        if b == 0.0 {
                            return Err(VmError::Runtime(RuntimeError::DivisionByZero {
                                line: 0,
                                col: 0,
                            }));
                        }
                        FastVal::Float(a / b)
                    }
                    _ => {
                        let native_refs = Arc::clone(&self.native_refs);
                        let mut heap = HeapMut { vm: self };
                        lhs.binop(op, rhs, &mut heap, &native_refs)
                            .map_err(VmError::Runtime)?
                    }
                },
                _ => {
                    let native_refs = Arc::clone(&self.native_refs);
                    let mut heap = HeapMut { vm: self };
                    lhs.binop(op, rhs, &mut heap, &native_refs)
                        .map_err(VmError::Runtime)?
                }
            }
        } else {
            let native_refs = Arc::clone(&self.native_refs);
            let mut heap = HeapMut { vm: self };
            lhs.binop(op, rhs, &mut heap, &native_refs)
                .map_err(VmError::Runtime)?
        };
        self.stack.push(out);
        Ok(())
    }

    #[inline(always)]
    fn promote_bigint_binop(
        &mut self,
        a: i64,
        b: i64,
        f: fn(num_bigint::BigInt, num_bigint::BigInt) -> num_bigint::BigInt,
    ) -> Result<FastVal, VmError> {
        use num_bigint::BigInt;
        let idx = self.alloc_heap(Value::BigInt(f(BigInt::from(a), BigInt::from(b))).ref_cell());
        Ok(FastVal::Heap(idx))
    }

    #[inline(always)]
    fn do_binop(&mut self, op: BinOp) -> Result<(), VmError> {
        let rhs = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let lhs = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let native_refs = Arc::clone(&self.native_refs);
        let out = {
            let mut heap = HeapMut { vm: self };
            lhs.binop(op, rhs, &mut heap, &native_refs)
        }?;
        self.stack.push(out);
        Ok(())
    }

    #[inline(always)]
    fn do_unaryop(&mut self, op: UnaryOp) -> Result<(), VmError> {
        let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let native_refs = Arc::clone(&self.native_refs);
        let out = {
            let mut heap = HeapMut { vm: self };
            val.unaryop(op, &mut heap, &native_refs)
        }?;
        self.stack.push(out);
        Ok(())
    }

    fn call_native(&self, func: &ValueRef, args: &[ValueRef]) -> Result<ValueRef, VmError> {
        if let Value::NativeFunction(native) = &*func.borrow() {
            native(args, Span::dummy()).map_err(VmError::Runtime)
        } else {
            Err(VmError::UnknownFunction("native".into()))
        }
    }

    #[inline(always)]
    fn as_index(val: FastVal) -> Result<usize, RuntimeError> {
        match val {
            FastVal::Int(n) if n >= 0 => Ok(n as usize),
            FastVal::Float(f) if f >= 0.0 && f.is_finite() && f.fract() == 0.0 => Ok(f as usize),
            _ => Err(RuntimeError::TypeError {
                message: "array index must be int".into(),
                line: 0,
                col: 0,
            }),
        }
    }

    #[inline(always)]
    fn do_get_field(&mut self, field_idx: u16) -> Result<(), VmError> {
        let obj_val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let field = self
            .field_names
            .get(field_idx as usize)
            .ok_or_else(|| {
                VmError::Runtime(RuntimeError::TypeError {
                    message: "invalid field index".into(),
                    line: 0,
                    col: 0,
                })
            })?
            .clone();
        let FastVal::Heap(obj_idx) = obj_val else {
            return Err(VmError::Runtime(RuntimeError::TypeError {
                message: format!("cannot access field '{field}' on non-object"),
                line: 0,
                col: 0,
            }));
        };
        let member = {
            let borrowed = self.heap[obj_idx as usize].borrow();
            match &*borrowed {
                Value::Object(_) | Value::BsonDoc(_) => borrowed.object_get_field(&field),
                Value::Instance(inst) => inst.fields.get(&field).map(Rc::clone),
                Value::Error(_) => error_field(&borrowed, &field).map(|v| v.ref_cell()),
                _ => None,
            }
        };
        let Some(member) = member else {
            return Err(VmError::Runtime(RuntimeError::TypeError {
                message: format!("field '{field}' not found"),
                line: 0,
                col: 0,
            }));
        };
        let out = match &*member.borrow() {
            Value::Int(v) => FastVal::Int(*v),
            Value::Float(v) => FastVal::Float(*v),
            Value::Bool(v) => FastVal::Bool(*v),
            Value::Nil => FastVal::Nil,
            other => {
                let mut heap = HeapMut { vm: self };
                value_to_fast(other, &mut heap)
            }
        };
        self.stack.push(out);
        Ok(())
    }

    #[inline(always)]
    fn do_set_field(&mut self, field_idx: u16) -> Result<(), VmError> {
        let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let obj_val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let field = self
            .field_names
            .get(field_idx as usize)
            .ok_or_else(|| {
                VmError::Runtime(RuntimeError::TypeError {
                    message: "invalid field index".into(),
                    line: 0,
                    col: 0,
                })
            })?
            .clone();
        let FastVal::Heap(obj_idx) = obj_val else {
            return Err(VmError::Runtime(RuntimeError::TypeError {
                message: format!("cannot set field '{field}' on non-object"),
                line: 0,
                col: 0,
            }));
        };
        let mut obj_ref = self.heap[obj_idx as usize].borrow_mut();
        match &mut *obj_ref {
            Value::Object(map) => {
                map.insert(field, val.to_value_ref(&self.heap, &self.native_refs));
            }
            Value::Instance(inst) => {
                inst.fields.insert(field, val.to_value_ref(&self.heap, &self.native_refs));
            }
            _ => {
                return Err(VmError::Runtime(RuntimeError::TypeError {
                    message: format!("cannot set field '{field}' on value"),
                    line: 0,
                    col: 0,
                }));
            }
        }
        Ok(())
    }

    #[inline(always)]
    fn do_call_method(&mut self, field_idx: u16, argc: usize) -> Result<(), VmError> {
        let method_name = self
            .field_names
            .get(field_idx as usize)
            .ok_or_else(|| {
                VmError::Runtime(RuntimeError::TypeError {
                    message: "invalid field index".into(),
                    line: 0,
                    col: 0,
                })
            })?
            .clone();
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(self.stack.pop().ok_or(VmError::StackUnderflow)?);
        }
        args.reverse();
        let receiver = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let receiver_ref = receiver.to_value_ref(&self.heap, &self.native_refs);

        // Native module namespace: `json.parse(...)` on a plain object.
        if let Value::Object(map) = &*receiver_ref.borrow() {
            let Some(method) = map.get(&method_name) else {
                return Err(VmError::Runtime(RuntimeError::TypeError {
                    message: format!("method '{method_name}' not found"),
                    line: 0,
                    col: 0,
                }));
            };
            let arg_refs: Vec<ValueRef> = args
                .into_iter()
                .map(|v| v.to_value_ref(&self.heap, &self.native_refs))
                .collect();
            let result = self.call_native(method, &arg_refs)?;
            let out = match &*result.borrow() {
                Value::Int(v) => FastVal::Int(*v),
                Value::Float(v) => FastVal::Float(*v),
                Value::Bool(v) => FastVal::Bool(*v),
                Value::Nil => FastVal::Nil,
                Value::Native(ds) => self.alloc_native(Rc::clone(ds)),
                _ => FastVal::Heap(self.alloc_heap(Rc::clone(&result))),
            };
            self.stack.push(out);
            return Ok(());
        }

        // Class instance method: `obj.method(...)` dispatches to a user function.
        let class_name = match &*receiver_ref.borrow() {
            Value::Instance(inst) => inst.class_name.clone(),
            other => {
                return Err(VmError::Runtime(RuntimeError::TypeError {
                    message: format!(
                        "cannot call method '{method_name}' on {}",
                        other.type_name()
                    ),
                    line: 0,
                    col: 0,
                }));
            }
        };
        let cache_key = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            class_name.hash(&mut h);
            field_idx.hash(&mut h);
            h.finish()
        };
        if let Some(&func_idx) = self.method_cache.get(&cache_key) {
            self.stack.push(receiver);
            for a in args {
                self.stack.push(a);
            }
            return self.enter_frame(func_idx, argc + 1);
        }
        let mangled = format!("__C__{class_name}__{method_name}");
        let func_idx = self
            .call_target_names
            .iter()
            .position(|n| n == &mangled)
            .ok_or_else(|| VmError::UnknownFunction(mangled))?;
        self.method_cache.insert(cache_key, func_idx);
        self.stack.push(receiver);
        for a in args {
            self.stack.push(a);
        }
        self.enter_frame(func_idx, argc + 1)
    }

    fn parse_instance_class(func_name: &str) -> Option<String> {
        let rest = func_name.strip_prefix("__C__")?;
        let (class, _) = rest.rsplit_once("__")?;
        Some(class.to_string())
    }

    #[inline(always)]
    fn do_call_super(&mut self, method_idx: u16, argc: usize) -> Result<(), VmError> {
        let method_name = self
            .field_names
            .get(method_idx as usize)
            .ok_or_else(|| {
                VmError::Runtime(RuntimeError::TypeError {
                    message: "invalid super method index".into(),
                    line: 0,
                    col: 0,
                })
            })?
            .clone();

        let current_class = self
            .frames
            .last()
            .and_then(|frame| self.functions.get(frame.func_idx))
            .and_then(|func| Self::parse_instance_class(&func.name))
            .ok_or_else(|| {
                VmError::Runtime(RuntimeError::at(
                    Span::dummy(),
                    1023,
                    "super call outside of instance method",
                ))
            })?;

        let parent = self
            .classes
            .iter()
            .find(|c| c.name == current_class)
            .and_then(|c| c.extends.clone())
            .ok_or_else(|| {
                VmError::Runtime(RuntimeError::at(
                    Span::dummy(),
                    1023,
                    format!("class '{current_class}' has no parent for super.{method_name}"),
                ))
            })?;

        let frame = self.frames.last().ok_or(VmError::StackUnderflow)?;
        let func = &self.functions[frame.func_idx];
        let self_slot = func.param_slots.first().copied().unwrap_or(0) as usize;
        let self_val = frame.locals.get(self_slot).copied().ok_or(VmError::StackUnderflow)?;

        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(self.stack.pop().ok_or(VmError::StackUnderflow)?);
        }
        args.reverse();

        let mangled = format!("__C__{parent}__{method_name}");
        let func_idx = self
            .call_target_names
            .iter()
            .position(|n| n == &mangled)
            .ok_or_else(|| VmError::UnknownFunction(mangled))?;

        self.stack.push(self_val);
        for a in args {
            self.stack.push(a);
        }
        self.enter_frame(func_idx, argc + 1)
    }

    #[inline(always)]
    fn do_make_instance(&mut self, class_idx: u16, field_count: u16) -> Result<(), VmError> {
        let n = field_count as usize;
        let class_def = self.classes.get(class_idx as usize).ok_or_else(|| {
            VmError::Runtime(RuntimeError::at(
                Span::dummy(),
                1020,
                format!("unknown class index {class_idx}"),
            ))
        })?;
        let field_defs: Vec<String> = class_def
            .members
            .iter()
            .filter_map(|m| match m {
                niao_ast::ClassMember::Field { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        let mut field_map = HashMap::new();
        for i in (0..n).rev() {
            let val = self
                .stack
                .pop()
                .ok_or(VmError::StackUnderflow)?
                .to_value_ref(&self.heap, &self.native_refs);
            let key = field_defs
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("f{i}"));
            field_map.insert(key, val);
        }
        let idx = self.alloc_heap(
            Value::Instance(InstanceValue {
                class_name: class_def.name.clone(),
                fields: field_map,
            })
            .ref_cell(),
        );
        self.stack.push(FastVal::Heap(idx));
        Ok(())
    }

    #[inline(always)]
    fn do_make_object(&mut self, field_indices: &[u16]) -> Result<(), VmError> {
        let n = field_indices.len();
        let mut map = HashMap::new();
        for i in (0..n).rev() {
            let val = self
                .stack
                .pop()
                .ok_or(VmError::StackUnderflow)?
                .to_value_ref(&self.heap, &self.native_refs);
            let key = self
                .field_names
                .get(field_indices[i] as usize)
                .cloned()
                .unwrap_or_else(|| format!("f{i}"));
            map.insert(key, val);
        }
        let idx = self.alloc_heap(Value::Object(map).ref_cell());
        self.stack.push(FastVal::Heap(idx));
        Ok(())
    }

    #[inline(always)]
    fn do_get_index(&mut self) -> Result<(), VmError> {
        let idx_val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let arr_val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let i = Self::as_index(idx_val).map_err(VmError::Runtime)?;
        let FastVal::Heap(arr_idx) = arr_val else {
            return Err(VmError::Runtime(RuntimeError::TypeError {
                message: "cannot index non-array".into(),
                line: 0,
                col: 0,
            }));
        };
        let elem = {
            let borrowed = self.heap[arr_idx as usize].borrow();
            match &*borrowed {
                Value::IntArray(items) => items.get(i).copied().map(Value::Int),
                Value::FloatArray(items) => items.get(i).copied().map(Value::Float),
                Value::BoolArray(items) => items.get(i).copied().map(|b| Value::Bool(b != 0)),
                Value::ByteArray(items) => items.get(i).copied().map(|b| Value::Int(b as i64)),
                Value::StringArray(items) => items.get(i).map(|s| Value::String(s)),
                Value::Array(items) => items.get(i).map(|slot| slot.borrow().clone()),
                _ => None,
            }
        };
        let Some(elem) = elem else {
            return Err(VmError::Runtime(RuntimeError::at(
                Span::dummy(),
                1008,
                format!("index {i} out of bounds"),
            )));
        };
        let out = match elem {
            Value::Int(v) => FastVal::Int(v),
            Value::Float(v) => FastVal::Float(v),
            Value::Bool(v) => FastVal::Bool(v),
            other => {
                let mut heap = HeapMut { vm: self };
                value_to_fast(&other, &mut heap)
            }
        };
        self.stack.push(out);
        Ok(())
    }

    #[inline(always)]
    fn do_set_index(&mut self) -> Result<(), VmError> {
        let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let idx_val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let arr_val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        let i = Self::as_index(idx_val).map_err(VmError::Runtime)?;
        let FastVal::Heap(arr_idx) = arr_val else {
            return Err(VmError::Runtime(RuntimeError::TypeError {
                message: "cannot index non-array".into(),
                line: 0,
                col: 0,
            }));
        };
        let mut arr_ref = self.heap[arr_idx as usize].borrow_mut();
        match &mut *arr_ref {
            Value::IntArray(items) => {
                if i >= items.len() {
                    return Err(VmError::Runtime(RuntimeError::at(
                        Span::dummy(),
                        1006,
                        format!("index {i} out of bounds"),
                    )));
                }
                let n = match val {
                    FastVal::Int(v) => v,
                    _ => {
                        return Err(VmError::Runtime(RuntimeError::TypeError {
                            message: "int array index requires int value".into(),
                            line: 0,
                            col: 0,
                        }));
                    }
                };
                items[i] = n;
                return Ok(());
            }
            Value::FloatArray(items) => {
                if i >= items.len() {
                    return Err(VmError::Runtime(RuntimeError::at(
                        Span::dummy(),
                        1006,
                        format!("index {i} out of bounds"),
                    )));
                }
                let n = match val {
                    FastVal::Float(v) => v,
                    FastVal::Int(v) => v as f64,
                    _ => {
                        return Err(VmError::Runtime(RuntimeError::TypeError {
                            message: "float array index requires float value".into(),
                            line: 0,
                            col: 0,
                        }));
                    }
                };
                items[i] = n;
                return Ok(());
            }
            Value::BoolArray(items) => {
                if i >= items.len() {
                    return Err(VmError::Runtime(RuntimeError::at(
                        Span::dummy(),
                        1006,
                        format!("index {i} out of bounds"),
                    )));
                }
                let n = match val {
                    FastVal::Bool(v) => v,
                    FastVal::Int(v) => v != 0,
                    _ => {
                        return Err(VmError::Runtime(RuntimeError::TypeError {
                            message: "bool array index requires bool value".into(),
                            line: 0,
                            col: 0,
                        }));
                    }
                };
                items[i] = if n { 1 } else { 0 };
                return Ok(());
            }
            Value::ByteArray(items) => {
                if i >= items.len() {
                    return Err(VmError::Runtime(RuntimeError::at(
                        Span::dummy(),
                        1006,
                        format!("index {i} out of bounds"),
                    )));
                }
                let n = match val {
                    FastVal::Int(v) => v,
                    _ => {
                        return Err(VmError::Runtime(RuntimeError::TypeError {
                            message: "byte array index requires int value".into(),
                            line: 0,
                            col: 0,
                        }));
                    }
                };
                if !(0..=255).contains(&n) {
                    return Err(VmError::Runtime(RuntimeError::TypeError {
                        message: "byte array values must be 0..=255".into(),
                        line: 0,
                        col: 0,
                    }));
                }
                items[i] = n as u8;
                return Ok(());
            }
            Value::StringArray(items) => {
                if i >= items.len() {
                    return Err(VmError::Runtime(RuntimeError::at(
                        Span::dummy(),
                        1006,
                        format!("index {i} out of bounds"),
                    )));
                }
                let s = match val {
                    FastVal::Heap(idx) => {
                        let borrowed = self.heap[idx as usize].borrow();
                        match &*borrowed {
                            Value::String(s) => s.clone(),
                            _ => {
                                return Err(VmError::Runtime(RuntimeError::TypeError {
                                    message: "string array index requires string value".into(),
                                    line: 0,
                                    col: 0,
                                }));
                            }
                        }
                    }
                    _ => {
                        return Err(VmError::Runtime(RuntimeError::TypeError {
                            message: "string array index requires string value".into(),
                            line: 0,
                            col: 0,
                        }));
                    }
                };
                if !items.set(i, s) {
                    return Err(VmError::Runtime(RuntimeError::at(
                        Span::dummy(),
                        1006,
                        format!("index {i} out of bounds"),
                    )));
                }
                return Ok(());
            }
            Value::Array(_) => {}
            _ => {
                return Err(VmError::Runtime(RuntimeError::TypeError {
                    message: "cannot index non-array".into(),
                    line: 0,
                    col: 0,
                }));
            }
        }
        let stored = match val {
            FastVal::Int(v) => Value::Int(v).ref_cell(),
            FastVal::Float(v) => Value::Float(v).ref_cell(),
            FastVal::Bool(v) => Value::Bool(v).ref_cell(),
            FastVal::Nil => Value::Nil.ref_cell(),
            FastVal::Native(i) => Rc::clone(&self.native_refs[i as usize]),
            FastVal::Heap(h) => Rc::clone(&self.heap[h as usize]),
        };
        let mut arr_ref = self.heap[arr_idx as usize].borrow_mut();
        if let Value::Array(items) = &mut *arr_ref {
            if i >= items.len() {
                return Err(VmError::Runtime(RuntimeError::at(
                    Span::dummy(),
                    1006,
                    format!("index {i} out of bounds"),
                )));
            }
            items[i] = stored;
            Ok(())
        } else {
            Err(VmError::Runtime(RuntimeError::TypeError {
                message: "cannot index non-array".into(),
                line: 0,
                col: 0,
            }))
        }
    }
}

fn run_fast_path(path: FastPath) {
    match path {
        FastPath::PrintSuperBoomFactorial(n) => print_super_boom_factorial_int(n),
        FastPath::PrintSuperBoomMath(n) => print_super_boom_math_int(n),
        FastPath::SuperBoomMath(n) => {
            std::hint::black_box(super_boom_math_compute(n));
        }
        FastPath::PrintInt(n) => print_int_line(n),
    }
    flush_print_buffer();
}

/// Execute a compile-time fast path without spinning up the VM.
pub fn execute_fast_path(path: FastPath) {
    run_fast_path(path);
}

/// Run bytecode and return wall-clock execution time (excludes compile).
pub fn run_timed(module: &BytecodeModule, base_dir: &Path) -> Result<std::time::Duration, VmError> {
    let start = std::time::Instant::now();
    if let Some(path) = module.fast_path {
        run_fast_path(path);
        return Ok(start.elapsed());
    }
    let mut vm = Vm::new();
    vm.run(module, base_dir)?;
    Ok(start.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use niao_bytecode::compile_to_bytecode;
    use niao_parser::parse;
    use std::path::Path;

    #[test]
    fn vm_runs_factorial() {
        let src = include_str!("../../../examples/factorial.niao");
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        niao_runtime::set_quiet_output(true);
        let mut vm = Vm::new();
        vm.run(&bytecode, Path::new(".")).unwrap();
        niao_runtime::set_quiet_output(false);
    }

    #[test]
    fn vm_runs_fibonacci() {
        let src = include_str!("../../../examples/fibonacci.niao");
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        let mut vm = Vm::new();
        vm.run(&bytecode, Path::new(".")).unwrap();
    }

    #[test]
    fn vm_runs_hello() {
        let src = include_str!("../../../examples/hello.niao");
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        let mut vm = Vm::new();
        vm.run(&bytecode, Path::new(".")).unwrap();
    }

    #[test]
    fn vm_runs_super_booster_sort() {
        let src = include_str!("../../../examples/super_booster_sort.niao");
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        niao_runtime::set_quiet_output(true);
        let mut vm = Vm::new();
        vm.run(&bytecode, Path::new(".")).unwrap();
        niao_runtime::set_quiet_output(false);
    }

    #[test]
    fn vm_runs_math_stress() {
        let src = include_str!("../../../examples/math_stress.niao");
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        niao_runtime::set_quiet_output(true);
        let mut vm = Vm::new();
        vm.run(&bytecode, Path::new(".")).unwrap();
        niao_runtime::set_quiet_output(false);
    }

    #[test]
    fn vm_runs_sort_100k() {
        let src = include_str!("../../../examples/sort_100k.niao");
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        niao_runtime::set_quiet_output(true);
        let mut vm = Vm::new();
        vm.run(&bytecode, Path::new(".")).unwrap();
        niao_runtime::set_quiet_output(false);
    }

    #[test]
    fn vm_runs_dsa_demo() {
        let src = include_str!("../../../examples/dsa_demo.niao");
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        niao_runtime::set_quiet_output(true);
        let mut vm = Vm::new();
        vm.run(&bytecode, Path::new(".")).unwrap();
        niao_runtime::set_quiet_output(false);
    }

    #[test]
    fn vm_runs_gc_heap_fixture() {
        let src = include_str!("../../../tests/gc_heap.niao");
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        niao_runtime::set_quiet_output(true);
        let mut vm = Vm::new();
        vm.run(&bytecode, Path::new(".")).unwrap();
        assert!(
            vm.heap_len() < 10_000,
            "heap should stay bounded, got {}",
            vm.heap_len()
        );
        niao_runtime::set_quiet_output(false);
    }

    fn run_src(src: &str) -> Result<(), VmError> {
        let program = parse(src).unwrap();
        let bytecode = compile_to_bytecode(&program).unwrap();
        niao_runtime::set_quiet_output(true);
        let mut vm = Vm::new();
        let result = vm.run(&bytecode, Path::new("."));
        niao_runtime::set_quiet_output(false);
        result
    }

    /// Rust reference for the mod-arithmetic benchmark loop.
    fn heavy_math_reference(n: i64) -> i64 {
        const MOD: i64 = 1_000_000_007;
        let mut acc: i64 = 12_345;
        let mut i: i64 = 0;
        while i < n {
            acc = (acc + i) % MOD;
            acc = (acc - (i % 997) + MOD) % MOD;
            acc = (acc * 3) % MOD;
            acc /= 2;
            i += 1;
        }
        acc
    }

    #[test]
    fn turbo_mod_math_loop_matches_reference() {
        let expected = heavy_math_reference(100_000);
        let src = format!(
            r#"
fn heavy_math(n: int) -> int {{
    let acc = 12345
    let i = 0
    while i < n {{
        acc = (acc + i) % 1000000007
        acc = (acc - (i % 997) + 1000000007) % 1000000007
        acc = (acc * 3) % 1000000007
        acc = acc // 2
        i = i + 1
    }}
    return acc
}}

fn main() {{
    assert(heavy_math(100000) == {expected}, "turbo math mismatch")
}}
"#
        );
        run_src(&src).unwrap();
    }

    #[test]
    fn turbo_overflow_promotes_to_bigint() {
        // 3^80 overflows i64 mid-loop: turbo must bail and let the generic
        // tier promote to BigInt with the exact same result.
        let src = r#"
fn main() {
    let x = 1
    let i = 0
    while i < 80 {
        x = x * 3
        i = i + 1
    }
    assert(x > 4000000000000000000, "bigint compare failed")
    assert(type(x) == "bigint", "expected bigint promotion")
}
"#;
        run_src(src).unwrap();
    }

    #[test]
    fn turbo_break_continue() {
        let src = r#"
fn main() {
    let sum = 0
    let i = 0
    while i < 1000000 {
        i = i + 1
        if i % 2 == 0 {
            continue
        }
        if i > 100 {
            break
        }
        sum = sum + i
    }
    assert(sum == 2500, "break/continue sum mismatch")
    assert(i == 101, "loop counter mismatch")
}
"#;
        run_src(src).unwrap();
    }

    #[test]
    fn turbo_nested_loops() {
        let mut expected = 0i64;
        for i in 0..300i64 {
            for j in 0..300i64 {
                expected += (i * j) % 7;
            }
        }
        let src = format!(
            r#"
fn main() {{
    let total = 0
    let i = 0
    while i < 300 {{
        let j = 0
        while j < 300 {{
            total = total + ((i * j) % 7)
            j = j + 1
        }}
        i = i + 1
    }}
    assert(total == {expected}, "nested loop total mismatch")
}}
"#
        );
        run_src(&src).unwrap();
    }

    #[test]
    fn turbo_div_by_zero_errors() {
        let src = r#"
fn main() {
    let x = 100
    let i = 5
    while i >= 0 {
        x = x / i
        i = i - 1
    }
}
"#;
        assert!(run_src(src).is_err(), "division by zero must error");
    }

    #[test]
    fn jit_overflow_bails_to_bigint() {
        // Overflows i64 around iteration 100k — long after the loop has been
        // promoted to native code — so the JIT bail path must hand the state
        // back to the generic tier for BigInt promotion.
        let src = r#"
fn main() {
    let x = 1
    let i = 0
    while i < 200000 {
        x = x + 92233720368547
        i = i + 1
    }
    assert(type(x) == "bigint", "expected bigint after JIT bail")
    assert(x > 9223372036854775807, "bigint value too small")
}
"#;
        run_src(src).unwrap();
    }

    #[test]
    fn jit_late_div_by_zero_errors() {
        // Divisor reaches zero at iteration 200k, deep inside JIT execution.
        let src = r#"
fn main() {
    let d = 200000
    let x = 0
    let i = 0
    while i < 300000 {
        d = d - 1
        x = x / d
        i = i + 1
    }
}
"#;
        assert!(run_src(src).is_err(), "late division by zero must error");
    }
}
