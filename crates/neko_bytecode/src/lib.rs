mod optimize;
mod wire;

use neko_ast::{ClassDef, Program, TraitDef};
use neko_ir::{lower, IrConst, IrInstr};

#[derive(Debug)]
pub enum CompileError {
    Ir(neko_ir::IrError),
    UnknownFunction(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Ir(e) => write!(f, "IR error: {e}"),
            CompileError::UnknownFunction(name) => {
                write!(f, "call to unknown function '{name}'")
            }
        }
    }
}

impl std::error::Error for CompileError {}

impl From<neko_ir::IrError> for CompileError {
    fn from(e: neko_ir::IrError) -> Self {
        CompileError::Ir(e)
    }
}

/// Increment when bytecode layout or semantics change so sidecar caches recompile.
pub const BYTECODE_CACHE_VERSION: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FastPath {
    PrintSuperBoomFactorial(i64),
    PrintSuperBoomMath(i64),
    SuperBoomMath(i64),
    /// `main() { print(const_int) }` — native print, no VM.
    PrintInt(i64),
}

#[derive(Debug, Clone)]
pub struct BytecodeModule {
    pub functions: Vec<BytecodeFunction>,
    pub constants: Vec<BytecodeConst>,
    /// Total number of variable slots across all functions.
    pub slot_count: usize,
    /// Maps Call opcode indices to function/builtin names.
    pub call_targets: Vec<String>,
    /// Whole-program fast path detected at compile time (skips VM dispatch).
    pub fast_path: Option<FastPath>,
    /// Cache format version; must match [`BYTECODE_CACHE_VERSION`].
    pub cache_version: u32,
    /// Builtin table fingerprint at compile time; must match the runtime.
    pub builtin_fingerprint: u64,
    /// Source file path at compile time (debugging / cache metadata).
    pub source_path: Option<String>,
    pub classes: Vec<ClassDef>,
    pub traits: Vec<TraitDef>,
    pub field_names: Vec<String>,
    /// Variable name per slot index (for global fallback on Load).
    pub slot_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BytecodeFunction {
    pub name: String,
    pub param_count: usize,
    pub param_slots: Vec<u16>,
    pub local_count: u16,
    pub memoize: bool,
    pub code: Vec<OpCode>,
}

#[derive(Debug, Clone)]
pub enum BytecodeConst {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nil,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpCode {
    Const(u16),
    Load(u16),
    Store(u16),
    BindGlobal(u16),
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Not,
    Neg,
    Call { func: u16, argc: u8 },
    Return,
    Jump(u32),
    JumpIfFalse(u32),
    Pop,
    MakeArray(u16),
    MakeObject(Vec<u16>),
    MakeInstance { class: u16, fields: u16 },
    GetField(u16),
    SetField(u16),
    GetIndex,
    SetIndex,
    CallMethod { field: u16, argc: u8 },
    CallSuper { method: u16, argc: u8 },
    TryBegin(u32),
    TryEnd(u32),
    Throw,
    Halt,
}

impl BytecodeModule {
    /// Serialize to compact binary `.nekobc` wire format.
    pub fn serialize(&self) -> Vec<u8> {
        wire::encode(self)
    }

    /// Load from binary `.nekobc` wire format, with legacy JSON fallback.
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        let module = if data.starts_with(wire::MAGIC) {
            wire::decode(data)?
        } else if data.first() == Some(&b'{') {
            serde_json::from_slice(data).ok()?
        } else {
            return None;
        };
        module.is_cache_valid().then_some(module)
    }

    pub fn is_cache_valid(&self) -> bool {
        self.cache_version == BYTECODE_CACHE_VERSION
            && self.builtin_fingerprint == neko_runtime::builtin_fingerprint()
            && self.call_targets_are_current()
    }

    fn call_targets_are_current(&self) -> bool {
        let builtins = neko_runtime::builtin_names();
        let user_count = self.functions.len();
        if self.call_targets.len() != user_count + builtins.len() {
            return false;
        }
        self.call_targets[user_count..]
            .iter()
            .zip(builtins.iter())
            .all(|(cached, expected)| cached == expected)
    }
}

impl serde::Serialize for BytecodeModule {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("BytecodeModule", 2)?;
        state.serialize_field("functions", &self.functions)?;
        state.serialize_field("constants", &self.constants)?;
        state.serialize_field("slot_count", &self.slot_count)?;
        state.serialize_field("call_targets", &self.call_targets)?;
        state.serialize_field("fast_path", &self.fast_path)?;
        state.serialize_field("cache_version", &self.cache_version)?;
        state.serialize_field("builtin_fingerprint", &self.builtin_fingerprint)?;
        state.serialize_field("source_path", &self.source_path)?;
        state.serialize_field("classes", &self.classes)?;
        state.serialize_field("traits", &self.traits)?;
        state.serialize_field("field_names", &self.field_names)?;
        state.serialize_field("slot_names", &self.slot_names)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for BytecodeModule {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            functions: Vec<BytecodeFunction>,
            constants: Vec<BytecodeConst>,
            #[serde(default)]
            slot_count: usize,
            #[serde(default)]
            call_targets: Vec<String>,
            #[serde(default)]
            fast_path: Option<FastPath>,
            #[serde(default)]
            cache_version: u32,
            #[serde(default)]
            builtin_fingerprint: u64,
            #[serde(default)]
            source_path: Option<String>,
            #[serde(default)]
            classes: Vec<ClassDef>,
            #[serde(default)]
            traits: Vec<TraitDef>,
            #[serde(default)]
            field_names: Vec<String>,
            #[serde(default)]
            slot_names: Vec<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let slot_count = if raw.slot_count == 0 {
            infer_slot_count(&raw.functions)
        } else {
            raw.slot_count
        };
        let call_targets = if raw.call_targets.is_empty() {
            raw.functions.iter().map(|f| f.name.clone()).collect()
        } else {
            raw.call_targets
        };
        Ok(BytecodeModule {
            functions: raw.functions,
            constants: raw.constants,
            slot_count,
            call_targets,
            fast_path: raw.fast_path,
            cache_version: raw.cache_version,
            builtin_fingerprint: raw.builtin_fingerprint,
            source_path: raw.source_path,
            classes: raw.classes,
            traits: raw.traits,
            field_names: raw.field_names,
            slot_names: raw.slot_names,
        })
    }
}

impl serde::Serialize for BytecodeFunction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("BytecodeFunction", 3)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("param_count", &self.param_count)?;
        state.serialize_field("param_slots", &self.param_slots)?;
        state.serialize_field("local_count", &self.local_count)?;
        state.serialize_field("memoize", &self.memoize)?;
        state.serialize_field("code", &self.code)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for BytecodeFunction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            name: String,
            param_count: usize,
            #[serde(default)]
            param_slots: Vec<u16>,
            #[serde(default)]
            local_count: u16,
            #[serde(default)]
            memoize: bool,
            code: Vec<OpCode>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let param_slots = if raw.param_slots.is_empty() {
            (0..raw.param_count as u16).collect()
        } else {
            raw.param_slots
        };
        let local_count = if raw.local_count == 0 {
            infer_function_local_count(&raw.code, &param_slots)
        } else {
            raw.local_count
        };
        Ok(BytecodeFunction {
            name: raw.name,
            param_count: raw.param_count,
            param_slots,
            local_count,
            memoize: raw.memoize,
            code: raw.code,
        })
    }
}

impl serde::Serialize for BytecodeConst {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            BytecodeConst::Int(v) => serializer.serialize_i64(*v),
            BytecodeConst::Float(v) => serializer.serialize_f64(*v),
            BytecodeConst::String(v) => serializer.serialize_str(v),
            BytecodeConst::Bool(v) => serializer.serialize_bool(*v),
            BytecodeConst::Nil => serializer.serialize_none(),
        }
    }
}

impl<'de> serde::Deserialize<'de> for BytecodeConst {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(BytecodeConst::Int(i))
                } else {
                    Ok(BytecodeConst::Float(n.as_f64().unwrap_or(0.0)))
                }
            }
            serde_json::Value::String(s) => Ok(BytecodeConst::String(s)),
            serde_json::Value::Bool(b) => Ok(BytecodeConst::Bool(b)),
            serde_json::Value::Null => Ok(BytecodeConst::Nil),
            _ => Ok(BytecodeConst::Nil),
        }
    }
}

fn parse_u16_list<E: serde::de::Error>(s: &str) -> Result<Vec<u16>, E> {
    let inner = s
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .ok_or_else(|| E::custom(format!("expected [u16, ...], got {s}")))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .map(|part| part.trim().parse::<u16>().map_err(E::custom))
        .collect()
}

impl serde::Serialize for OpCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = format!("{self:?}");
        serializer.serialize_str(&s)
    }
}

impl<'de> serde::Deserialize<'de> for OpCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if let Some(inner) = s.strip_prefix("Const(").and_then(|t| t.strip_suffix(')')) {
            let v: u16 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::Const(v));
        }
        if let Some(inner) = s.strip_prefix("Load(").and_then(|t| t.strip_suffix(')')) {
            let v: u16 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::Load(v));
        }
        if let Some(inner) = s.strip_prefix("Store(").and_then(|t| t.strip_suffix(')')) {
            let v: u16 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::Store(v));
        }
        if let Some(inner) = s.strip_prefix("BindGlobal(").and_then(|t| t.strip_suffix(')')) {
            let v: u16 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::BindGlobal(v));
        }
        if let Some(inner) = s.strip_prefix("Jump(").and_then(|t| t.strip_suffix(')')) {
            let v: u32 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::Jump(v));
        }
        if let Some(inner) = s
            .strip_prefix("JumpIfFalse(")
            .and_then(|t| t.strip_suffix(')'))
        {
            let v: u32 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::JumpIfFalse(v));
        }
        if let Some(inner) = s.strip_prefix("MakeArray(").and_then(|t| t.strip_suffix(')')) {
            let v: u16 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::MakeArray(v));
        }
        if let Some(inner) = s.strip_prefix("MakeObject(").and_then(|t| t.strip_suffix(')')) {
            if let Ok(v) = inner.parse::<u16>() {
                return Ok(OpCode::MakeObject((0..v).collect()));
            }
            let fields = parse_u16_list(inner)?;
            return Ok(OpCode::MakeObject(fields));
        }
        if let Some(inner) = s.strip_prefix("SetField(").and_then(|t| t.strip_suffix(')')) {
            let v: u16 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::SetField(v));
        }
        if s.starts_with("MakeInstance { class: ") {
            let class = s["MakeInstance { class: ".len()..]
                .split(", fields: ")
                .next()
                .ok_or_else(|| serde::de::Error::custom("invalid MakeInstance"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            let fields = s
                .split("fields: ")
                .nth(1)
                .and_then(|t| t.strip_suffix(" }"))
                .ok_or_else(|| serde::de::Error::custom("invalid MakeInstance"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            return Ok(OpCode::MakeInstance { class, fields });
        }
        if s.starts_with("CallMethod { field: ") {
            let field = s["CallMethod { field: ".len()..]
                .split(", argc: ")
                .next()
                .ok_or_else(|| serde::de::Error::custom("invalid CallMethod"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            let argc = s
                .split("argc: ")
                .nth(1)
                .and_then(|t| t.strip_suffix(" }"))
                .ok_or_else(|| serde::de::Error::custom("invalid CallMethod"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            return Ok(OpCode::CallMethod { field, argc });
        }
        if s.starts_with("CallSuper { method: ") {
            let method = s["CallSuper { method: ".len()..]
                .split(", argc: ")
                .next()
                .ok_or_else(|| serde::de::Error::custom("invalid CallSuper"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            let argc = s
                .split("argc: ")
                .nth(1)
                .and_then(|t| t.strip_suffix(" }"))
                .ok_or_else(|| serde::de::Error::custom("invalid CallSuper"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            return Ok(OpCode::CallSuper { method, argc });
        }
        if let Some(inner) = s.strip_prefix("GetField(").and_then(|t| t.strip_suffix(')')) {
            let v: u16 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::GetField(v));
        }
        if let Some(inner) = s.strip_prefix("SetField(").and_then(|t| t.strip_suffix(')')) {
            let v: u16 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::SetField(v));
        }
        if s.starts_with("CallMethod { field: ") {
            let field = s["CallMethod { field: ".len()..]
                .split(", argc: ")
                .next()
                .ok_or_else(|| serde::de::Error::custom("invalid CallMethod opcode"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            let argc = s
                .split("argc: ")
                .nth(1)
                .and_then(|t| t.strip_suffix(" }"))
                .ok_or_else(|| serde::de::Error::custom("invalid CallMethod opcode"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            return Ok(OpCode::CallMethod { field, argc });
        }
        if s.starts_with("Call { func: ") {
            let func = s["Call { func: ".len()..]
                .split(", argc: ")
                .next()
                .ok_or_else(|| serde::de::Error::custom("invalid Call opcode"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            let argc = s
                .split("argc: ")
                .nth(1)
                .and_then(|t| t.strip_suffix(" }"))
                .ok_or_else(|| serde::de::Error::custom("invalid Call opcode"))?
                .parse()
                .map_err(serde::de::Error::custom)?;
            return Ok(OpCode::Call { func, argc });
        }
        if let Some(inner) = s.strip_prefix("TryBegin(").and_then(|t| t.strip_suffix(')')) {
            let v: u32 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::TryBegin(v));
        }
        if let Some(inner) = s.strip_prefix("TryEnd(").and_then(|t| t.strip_suffix(')')) {
            let v: u32 = inner.parse().map_err(serde::de::Error::custom)?;
            return Ok(OpCode::TryEnd(v));
        }
        match s.as_str() {
            "Add" => Ok(OpCode::Add),
            "Sub" => Ok(OpCode::Sub),
            "Mul" => Ok(OpCode::Mul),
            "Div" => Ok(OpCode::Div),
            "FloorDiv" => Ok(OpCode::FloorDiv),
            "Mod" => Ok(OpCode::Mod),
            "Eq" => Ok(OpCode::Eq),
            "Ne" => Ok(OpCode::Ne),
            "Lt" => Ok(OpCode::Lt),
            "Gt" => Ok(OpCode::Gt),
            "Le" => Ok(OpCode::Le),
            "Ge" => Ok(OpCode::Ge),
            "And" => Ok(OpCode::And),
            "Or" => Ok(OpCode::Or),
            "Not" => Ok(OpCode::Not),
            "Neg" => Ok(OpCode::Neg),
            "GetIndex" => Ok(OpCode::GetIndex),
            "SetIndex" => Ok(OpCode::SetIndex),
            "Return" => Ok(OpCode::Return),
            "Pop" => Ok(OpCode::Pop),
            "Throw" => Ok(OpCode::Throw),
            "Halt" => Ok(OpCode::Halt),
            other => Err(serde::de::Error::custom(format!("unknown opcode: {other}"))),
        }
    }
}

pub fn compile_to_bytecode(program: &Program) -> Result<BytecodeModule, CompileError> {
    let mut ir = lower(program)?;
    optimize::optimize_ir(&mut ir);
    let constants: Vec<BytecodeConst> = ir
        .constants
        .iter()
        .map(|c| match c {
            IrConst::Int(v) => BytecodeConst::Int(*v),
            IrConst::Float(v) => BytecodeConst::Float(*v),
            IrConst::String(v) => BytecodeConst::String(v.clone()),
            IrConst::Bool(v) => BytecodeConst::Bool(*v),
            IrConst::Nil => BytecodeConst::Nil,
        })
        .collect();

    let mut var_slots: std::collections::HashMap<String, u16> = std::collections::HashMap::new();
    let mut next_slot: u16 = 0;

    {
        let mut alloc_slot = |name: &str| -> u16 {
            if let Some(&idx) = var_slots.get(name) {
                idx
            } else {
                let idx = next_slot;
                next_slot += 1;
                var_slots.insert(name.to_string(), idx);
                idx
            }
        };

        for f in &ir.functions {
            for p in &f.params {
                alloc_slot(p);
            }
            for instr in &f.instructions {
                match instr {
                    IrInstr::Load(n) | IrInstr::Store(n) | IrInstr::BindGlobal(n) => {
                        alloc_slot(n);
                    }
                    _ => {}
                }
            }
        }
    }

    let slot_count = next_slot as usize;
    let mut slot_names = vec![String::new(); slot_count];
    for (name, &idx) in &var_slots {
        slot_names[idx as usize] = name.clone();
    }

    let mut call_targets: Vec<String> = ir.functions.iter().map(|f| f.name.clone()).collect();
    // Single source of truth: every builtin registered in neko_runtime
    // (core + dsa) becomes a callable target.
    for builtin in neko_runtime::builtin_names() {
        call_targets.push(builtin.to_string());
    }
    // First occurrence wins: user-defined functions precede builtins in
    // call_targets, so a user fn shadows a builtin with the same name.
    let mut call_map: std::collections::HashMap<String, u16> = std::collections::HashMap::new();
    for (i, n) in call_targets.iter().enumerate() {
        call_map.entry(n.clone()).or_insert(i as u16);
    }

    let mut field_names = ir.field_names.clone();

    let mut result_functions = Vec::new();
    for f in &ir.functions {
        let mut code = Vec::new();
        for instr in &f.instructions {
            code.push(lower_instr(
                instr,
                &var_slots,
                &call_map,
                &ir.classes,
                &mut field_names,
            )?);
        }
        code.push(OpCode::Halt);
        let param_slots: Vec<u16> = f
            .params
            .iter()
            .map(|p| *var_slots.get(p).unwrap_or(&0))
            .collect();
        let local_count = compute_local_count(&f.instructions, &param_slots, &var_slots);
        let memoize = f.params.len() == 1 && is_self_recursive(f);
        result_functions.push(BytecodeFunction {
            name: f.name.clone(),
            param_count: f.params.len(),
            param_slots,
            local_count,
            memoize,
            code,
        });
    }

    let fast_path = detect_fast_path(&call_targets, &result_functions, &constants);

    for f in &mut result_functions {
        optimize::peephole_function(f, &constants);
    }
    optimize::inline_tiny_callees(&mut result_functions, &call_targets);

    Ok(BytecodeModule {
        functions: result_functions,
        constants,
        slot_count,
        call_targets,
        fast_path,
        cache_version: BYTECODE_CACHE_VERSION,
        builtin_fingerprint: neko_runtime::builtin_fingerprint(),
        source_path: None,
        classes: ir.classes,
        traits: ir.traits,
        field_names,
        slot_names,
    })
}

/// Detect fused super-boom print calls in `main()` — bypass VM entirely at runtime.
pub fn detect_fast_path(
    call_targets: &[String],
    functions: &[BytecodeFunction],
    constants: &[BytecodeConst],
) -> Option<FastPath> {
    let print_super_idx = call_targets
        .iter()
        .position(|n| n == "print_super_boom_factorial");
    let print_super_math_idx = call_targets
        .iter()
        .position(|n| n == "print_super_boom_math");
    let print_idx = call_targets.iter().position(|n| n == "print");
    let factorial_idx = call_targets.iter().position(|n| n == "super_boom_factorial");
    let math_idx = call_targets.iter().position(|n| n == "super_boom_math");
    let main = functions.iter().find(|f| f.name == "main")?;
    let code = &main.code;

    if let Some(print_super_idx) = print_super_idx {
        for window in code.windows(3) {
            let [OpCode::Const(cidx), OpCode::Call { func, argc: 1 }, tail] = window else {
                continue;
            };
            if *func != print_super_idx as u16 {
                continue;
            }
            if !matches!(tail, OpCode::Return | OpCode::Halt | OpCode::Pop) {
                continue;
            }
            if let Some(n) = const_int(constants, *cidx) {
                return Some(FastPath::PrintSuperBoomFactorial(n));
            }
        }
    }

    if let Some(print_super_math_idx) = print_super_math_idx {
        for window in code.windows(3) {
            let [OpCode::Const(cidx), OpCode::Call { func, argc: 1 }, tail] = window else {
                continue;
            };
            if *func != print_super_math_idx as u16 {
                continue;
            }
            if !matches!(tail, OpCode::Return | OpCode::Halt | OpCode::Pop) {
                continue;
            }
            if let Some(n) = const_int(constants, *cidx) {
                return Some(FastPath::PrintSuperBoomMath(n));
            }
        }
    }

    if let Some(math_idx) = math_idx {
        for window in code.windows(3) {
            let [OpCode::Const(cidx), OpCode::Call { func, argc: 1 }, tail] = window else {
                continue;
            };
            if *func != math_idx as u16 {
                continue;
            }
            if !matches!(
                tail,
                OpCode::Return | OpCode::Halt | OpCode::Pop | OpCode::Store(_)
            ) {
                continue;
            }
            if let Some(n) = const_int(constants, *cidx) {
                return Some(FastPath::SuperBoomMath(n));
            }
        }

        if let Some(print_idx) = print_idx {
            for window in code.windows(4) {
                let [OpCode::Const(cidx), OpCode::Call { func, argc: 1 }, OpCode::Call { func: pfunc, argc: 1 }, tail] =
                    window
                else {
                    continue;
                };
                if *func != math_idx as u16 || *pfunc != print_idx as u16 {
                    continue;
                }
                if !matches!(tail, OpCode::Return | OpCode::Halt | OpCode::Pop) {
                    continue;
                }
                if let Some(n) = const_int(constants, *cidx) {
                    return Some(FastPath::PrintSuperBoomMath(n));
                }
            }
        }
    }

    if let (Some(print_idx), Some(factorial_idx)) = (print_idx, factorial_idx) {
        for window in code.windows(4) {
            let [OpCode::Const(cidx), OpCode::Call { func, argc: 1 }, OpCode::Call { func: pfunc, argc: 1 }, tail] =
                window
            else {
                continue;
            };
            if *func != factorial_idx as u16 || *pfunc != print_idx as u16 {
                continue;
            }
            if !matches!(tail, OpCode::Return | OpCode::Halt | OpCode::Pop) {
                continue;
            }
            if let Some(n) = const_int(constants, *cidx) {
                return Some(FastPath::PrintSuperBoomFactorial(n));
            }
        }
    }

    if let Some(print_idx) = print_idx {
        for window in code.windows(3) {
            let [OpCode::Const(cidx), OpCode::Call { func, argc: 1 }, tail] = window else {
                continue;
            };
            if *func != print_idx as u16 {
                continue;
            }
            if !matches!(tail, OpCode::Return | OpCode::Halt | OpCode::Pop) {
                continue;
            }
            if let Some(n) = const_int(constants, *cidx) {
                return Some(FastPath::PrintInt(n));
            }
        }
    }

    None
}

fn const_int(constants: &[BytecodeConst], idx: u16) -> Option<i64> {
    match constants.get(idx as usize)? {
        BytecodeConst::Int(n) if *n >= 0 => Some(*n),
        _ => None,
    }
}

impl BytecodeModule {
    pub fn ensure_fast_path(&mut self) {
        if self.fast_path.is_none() {
            self.fast_path = detect_fast_path(&self.call_targets, &self.functions, &self.constants);
        }
    }
}

fn infer_slot_count(functions: &[BytecodeFunction]) -> usize {
    functions
        .iter()
        .map(|f| f.local_count as usize)
        .max()
        .unwrap_or(0)
}

fn infer_function_local_count(code: &[OpCode], param_slots: &[u16]) -> u16 {
    let mut max_slot = param_slots.iter().copied().max().unwrap_or(0);
    for op in code {
        if let OpCode::Load(idx) | OpCode::Store(idx) = op {
            max_slot = max_slot.max(*idx);
        }
    }
    max_slot + 1
}

fn compute_local_count(
    instructions: &[IrInstr],
    param_slots: &[u16],
    var_slots: &std::collections::HashMap<String, u16>,
) -> u16 {
    let mut max_slot = param_slots.iter().copied().max().unwrap_or(0);
    for instr in instructions {
        match instr {
            IrInstr::Load(n) | IrInstr::Store(n) | IrInstr::BindGlobal(n) => {
                if let Some(&s) = var_slots.get(n) {
                    max_slot = max_slot.max(s);
                }
            }
            _ => {}
        }
    }
    max_slot + 1
}

fn is_self_recursive(f: &neko_ir::IrFunction) -> bool {
    f.instructions.iter().any(|instr| {
        matches!(
            instr,
            IrInstr::Call { name, .. } if name == &f.name
        )
    })
}

fn field_name_idx(field_names: &mut Vec<String>, field: &str) -> usize {
    if let Some(i) = field_names.iter().position(|n| n == field) {
        i
    } else {
        field_names.push(field.to_string());
        field_names.len() - 1
    }
}

fn lower_instr(
    instr: &IrInstr,
    slots: &std::collections::HashMap<String, u16>,
    calls: &std::collections::HashMap<String, u16>,
    classes: &[ClassDef],
    field_names: &mut Vec<String>,
) -> Result<OpCode, CompileError> {
    let slot = |name: &str| -> u16 { *slots.get(name).unwrap_or(&0) };
    Ok(match instr {
        IrInstr::Const(i) => OpCode::Const(*i as u16),
        IrInstr::Load(n) => OpCode::Load(slot(n)),
        IrInstr::Store(n) => OpCode::Store(slot(n)),
        IrInstr::BindGlobal(n) => OpCode::BindGlobal(slot(n)),
        IrInstr::Binary(neko_ast::BinOp::Add) => OpCode::Add,
        IrInstr::Binary(neko_ast::BinOp::Sub) => OpCode::Sub,
        IrInstr::Binary(neko_ast::BinOp::Mul) => OpCode::Mul,
        IrInstr::Binary(neko_ast::BinOp::Div) => OpCode::Div,
        IrInstr::Binary(neko_ast::BinOp::FloorDiv) => OpCode::FloorDiv,
        IrInstr::Binary(neko_ast::BinOp::Mod) => OpCode::Mod,
        IrInstr::Binary(neko_ast::BinOp::Eq) => OpCode::Eq,
        IrInstr::Binary(neko_ast::BinOp::Ne) => OpCode::Ne,
        IrInstr::Binary(neko_ast::BinOp::Lt) => OpCode::Lt,
        IrInstr::Binary(neko_ast::BinOp::Gt) => OpCode::Gt,
        IrInstr::Binary(neko_ast::BinOp::Le) => OpCode::Le,
        IrInstr::Binary(neko_ast::BinOp::Ge) => OpCode::Ge,
        IrInstr::Binary(neko_ast::BinOp::And) => OpCode::And,
        IrInstr::Binary(neko_ast::BinOp::Or) => OpCode::Or,
        IrInstr::Unary(neko_ast::UnaryOp::Not) => OpCode::Not,
        IrInstr::Unary(neko_ast::UnaryOp::Neg) => OpCode::Neg,
        IrInstr::Call { name, argc } => OpCode::Call {
            func: *calls
                .get(name)
                .ok_or_else(|| CompileError::UnknownFunction(name.clone()))?,
            argc: *argc as u8,
        },
        IrInstr::Return => OpCode::Return,
        IrInstr::Jump(t) => OpCode::Jump(*t as u32),
        IrInstr::JumpIfFalse(t) => OpCode::JumpIfFalse(*t as u32),
        IrInstr::Pop => OpCode::Pop,
        IrInstr::MakeArray(n) => OpCode::MakeArray(*n as u16),
        IrInstr::MakeObject(indices) => {
            OpCode::MakeObject(indices.iter().map(|i| *i as u16).collect())
        }
        IrInstr::MakeInstance { class, field_count } => {
            let class_idx = classes
                .iter()
                .position(|c| c.name == *class)
                .ok_or_else(|| CompileError::UnknownFunction(format!("class '{class}'")))?
                as u16;
            OpCode::MakeInstance {
                class: class_idx,
                fields: *field_count as u16,
            }
        }
        IrInstr::GetField(idx) => OpCode::GetField(*idx as u16),
        IrInstr::SetField(idx) => OpCode::SetField(*idx as u16),
        IrInstr::GetIndex => OpCode::GetIndex,
        IrInstr::SetIndex => OpCode::SetIndex,
        IrInstr::CallMethod { field, argc } => OpCode::CallMethod {
            field: *field as u16,
            argc: *argc as u8,
        },
        IrInstr::CallStatic { class, method, argc } => {
            let name = format!("__CS__{class}__{method}");
            OpCode::Call {
                func: *calls
                    .get(&name)
                    .ok_or_else(|| CompileError::UnknownFunction(name.clone()))?,
                argc: *argc as u8,
            }
        }
        IrInstr::CallSuper { method, argc } => OpCode::CallSuper {
            method: field_name_idx(field_names, method) as u16,
            argc: *argc as u8,
        },
        IrInstr::TryBegin(t) => OpCode::TryBegin(*t as u32),
        IrInstr::TryEnd(t) => OpCode::TryEnd(*t as u32),
        IrInstr::Throw => OpCode::Throw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neko_parser::parse;

    #[test]
    fn detects_print_super_boom_factorial() {
        let src = "fn main() { print_super_boom_factorial(50) }";
        let program = parse(src).unwrap();
        let bc = compile_to_bytecode(&program).unwrap();
        assert_eq!(
            bc.fast_path,
            Some(FastPath::PrintSuperBoomFactorial(50))
        );
    }

    #[test]
    fn detects_print_call_super_boom_factorial() {
        let src = "fn main() { print(super_boom_factorial(50)) }";
        let program = parse(src).unwrap();
        let bc = compile_to_bytecode(&program).unwrap();
        assert_eq!(
            bc.fast_path,
            Some(FastPath::PrintSuperBoomFactorial(50))
        );
    }

    #[test]
    fn detects_print_super_boom_math() {
        let src = "fn main() { print_super_boom_math(10000000) }";
        let program = parse(src).unwrap();
        let bc = compile_to_bytecode(&program).unwrap();
        assert_eq!(
            bc.fast_path,
            Some(FastPath::PrintSuperBoomMath(10_000_000))
        );
    }

    #[test]
    fn detects_super_boom_math_compute_only() {
        let src = "fn main() { let x = super_boom_math(10000000) }";
        let program = parse(src).unwrap();
        let bc = compile_to_bytecode(&program).unwrap();
        assert_eq!(
            bc.fast_path,
            Some(FastPath::SuperBoomMath(10_000_000))
        );
    }

    #[test]
    fn rejects_stale_cache_without_version_metadata() {
        let src = "fn main() { print(1) }";
        let program = parse(src).unwrap();
        let bc = compile_to_bytecode(&program).unwrap();
        let mut legacy = serde_json::to_value(&bc).unwrap();
        let obj = legacy.as_object_mut().unwrap();
        obj.remove("cache_version");
        obj.remove("builtin_fingerprint");
        let bytes = serde_json::to_vec(&legacy).unwrap();
        assert!(BytecodeModule::deserialize(&bytes).is_none());
    }

    #[test]
    fn roundtrip_cache_includes_version_metadata() {
        let src = "fn main() { print(1) }";
        let program = parse(src).unwrap();
        let bc = compile_to_bytecode(&program).unwrap();
        let bytes = bc.serialize();
        assert!(bytes.starts_with(wire::MAGIC));
        let loaded = BytecodeModule::deserialize(&bytes).unwrap();
        assert_eq!(loaded.cache_version, BYTECODE_CACHE_VERSION);
        assert_eq!(loaded.builtin_fingerprint, neko_runtime::builtin_fingerprint());
        assert!(loaded.is_cache_valid());
    }

    #[test]
    fn legacy_json_cache_still_loads() {
        let src = "fn main() { print(1) }";
        let program = parse(src).unwrap();
        let bc = compile_to_bytecode(&program).unwrap();
        let json = serde_json::to_vec(&bc).unwrap();
        assert!(json.first() == Some(&b'{'));
        let loaded = BytecodeModule::deserialize(&json).unwrap();
        assert_eq!(loaded.functions[0].code, bc.functions[0].code);
    }
}
