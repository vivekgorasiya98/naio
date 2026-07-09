use crate::{BytecodeConst, BytecodeFunction, BytecodeModule, FastPath, OpCode};
use niao_ast::{ClassDef, TraitDef};

pub(crate) const MAGIC: &[u8] = b"NIAOBC";
const WIRE_VERSION: u8 = 1;

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    fn write_u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    fn write_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn write_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn write_u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn write_i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn write_bool(&mut self, v: bool) {
        self.write_u8(u8::from(v));
    }

    fn write_string(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.write_u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }

    fn write_optional_string(&mut self, s: &Option<String>) {
        match s {
            None => self.write_u8(0),
            Some(v) => {
                self.write_u8(1);
                self.write_string(v);
            }
        }
    }

    fn write_blob(&mut self, data: &[u8]) {
        self.write_u32(data.len() as u32);
        self.buf.extend_from_slice(data);
    }

    fn write_fast_path(&mut self, fp: &Option<FastPath>) {
        match fp {
            None => self.write_u8(0),
            Some(FastPath::PrintSuperBoomFactorial(n)) => {
                self.write_u8(1);
                self.write_i64(*n);
            }
            Some(FastPath::PrintSuperBoomMath(n)) => {
                self.write_u8(2);
                self.write_i64(*n);
            }
            Some(FastPath::SuperBoomMath(n)) => {
                self.write_u8(3);
                self.write_i64(*n);
            }
            Some(FastPath::PrintInt(n)) => {
                self.write_u8(4);
                self.write_i64(*n);
            }
        }
    }

    fn write_const(&mut self, c: &BytecodeConst) {
        match c {
            BytecodeConst::Int(v) => {
                self.write_u8(0);
                self.write_i64(*v);
            }
            BytecodeConst::Float(v) => {
                self.write_u8(1);
                self.buf.extend_from_slice(&v.to_le_bytes());
            }
            BytecodeConst::String(v) => {
                self.write_u8(2);
                self.write_string(v);
            }
            BytecodeConst::Bool(v) => {
                self.write_u8(3);
                self.write_bool(*v);
            }
            BytecodeConst::Nil => self.write_u8(4),
        }
    }

    fn write_opcode(&mut self, op: &OpCode) {
        match op {
            OpCode::Const(v) => {
                self.write_u8(0);
                self.write_u16(*v);
            }
            OpCode::Load(v) => {
                self.write_u8(1);
                self.write_u16(*v);
            }
            OpCode::Store(v) => {
                self.write_u8(2);
                self.write_u16(*v);
            }
            OpCode::BindGlobal(v) => {
                self.write_u8(3);
                self.write_u16(*v);
            }
            OpCode::Add => self.write_u8(10),
            OpCode::Sub => self.write_u8(11),
            OpCode::Mul => self.write_u8(12),
            OpCode::Div => self.write_u8(13),
            OpCode::FloorDiv => self.write_u8(14),
            OpCode::Mod => self.write_u8(15),
            OpCode::Eq => self.write_u8(16),
            OpCode::Ne => self.write_u8(17),
            OpCode::Lt => self.write_u8(18),
            OpCode::Gt => self.write_u8(19),
            OpCode::Le => self.write_u8(20),
            OpCode::Ge => self.write_u8(21),
            OpCode::And => self.write_u8(22),
            OpCode::Or => self.write_u8(23),
            OpCode::Not => self.write_u8(24),
            OpCode::Neg => self.write_u8(25),
            OpCode::Call { func, argc } => {
                self.write_u8(30);
                self.write_u16(*func);
                self.write_u8(*argc);
            }
            OpCode::Return => self.write_u8(31),
            OpCode::Jump(v) => {
                self.write_u8(32);
                self.write_u32(*v);
            }
            OpCode::JumpIfFalse(v) => {
                self.write_u8(33);
                self.write_u32(*v);
            }
            OpCode::Pop => self.write_u8(34),
            OpCode::MakeArray(v) => {
                self.write_u8(40);
                self.write_u16(*v);
            }
            OpCode::MakeObject(fields) => {
                self.write_u8(41);
                self.write_u16(fields.len() as u16);
                for f in fields {
                    self.write_u16(*f);
                }
            }
            OpCode::MakeInstance { class, fields } => {
                self.write_u8(42);
                self.write_u16(*class);
                self.write_u16(*fields);
            }
            OpCode::GetField(v) => {
                self.write_u8(43);
                self.write_u16(*v);
            }
            OpCode::SetField(v) => {
                self.write_u8(44);
                self.write_u16(*v);
            }
            OpCode::GetIndex => self.write_u8(45),
            OpCode::SetIndex => self.write_u8(46),
            OpCode::CallMethod { field, argc } => {
                self.write_u8(47);
                self.write_u16(*field);
                self.write_u8(*argc);
            }
            OpCode::CallSuper { method, argc } => {
                self.write_u8(48);
                self.write_u16(*method);
                self.write_u8(*argc);
            }
            OpCode::TryBegin(v) => {
                self.write_u8(50);
                self.write_u32(*v);
            }
            OpCode::TryEnd(v) => {
                self.write_u8(51);
                self.write_u32(*v);
            }
            OpCode::Throw => self.write_u8(52),
            OpCode::Halt => self.write_u8(53),
        }
    }

    fn write_function(&mut self, f: &BytecodeFunction) {
        self.write_string(&f.name);
        self.write_u32(f.param_count as u32);
        self.write_u32(f.param_slots.len() as u32);
        for slot in &f.param_slots {
            self.write_u16(*slot);
        }
        self.write_u16(f.local_count);
        self.write_bool(f.memoize);
        self.write_u32(f.code.len() as u32);
        for op in &f.code {
            self.write_opcode(op);
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> Option<u8> {
        if self.pos >= self.data.len() {
            return None;
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Some(v)
    }

    fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.pos + n > self.data.len() {
            return None;
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Some(slice)
    }

    fn read_u16(&mut self) -> Option<u16> {
        let bytes = self.read_bytes(2)?;
        Some(u16::from_le_bytes(bytes.try_into().ok()?))
    }

    fn read_u32(&mut self) -> Option<u32> {
        let bytes = self.read_bytes(4)?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    }

    fn read_u64(&mut self) -> Option<u64> {
        let bytes = self.read_bytes(8)?;
        Some(u64::from_le_bytes(bytes.try_into().ok()?))
    }

    fn read_i64(&mut self) -> Option<i64> {
        let bytes = self.read_bytes(8)?;
        Some(i64::from_le_bytes(bytes.try_into().ok()?))
    }

    fn read_bool(&mut self) -> Option<bool> {
        Some(self.read_u8()? != 0)
    }

    fn read_string(&mut self) -> Option<String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_bytes(len)?;
        std::str::from_utf8(bytes).ok().map(str::to_owned)
    }

    fn read_optional_string(&mut self) -> Option<Option<String>> {
        match self.read_u8()? {
            0 => Some(None),
            1 => self.read_string().map(Some),
            _ => None,
        }
    }

    fn read_blob(&mut self) -> Option<Vec<u8>> {
        let len = self.read_u32()? as usize;
        Some(self.read_bytes(len)?.to_vec())
    }

    fn read_fast_path(&mut self) -> Option<Option<FastPath>> {
        match self.read_u8()? {
            0 => Some(None),
            1 => Some(Some(FastPath::PrintSuperBoomFactorial(self.read_i64()?))),
            2 => Some(Some(FastPath::PrintSuperBoomMath(self.read_i64()?))),
            3 => Some(Some(FastPath::SuperBoomMath(self.read_i64()?))),
            4 => Some(Some(FastPath::PrintInt(self.read_i64()?))),
            _ => None,
        }
    }

    fn read_const(&mut self) -> Option<BytecodeConst> {
        match self.read_u8()? {
            0 => Some(BytecodeConst::Int(self.read_i64()?)),
            1 => {
                let bytes = self.read_bytes(8)?;
                Some(BytecodeConst::Float(f64::from_le_bytes(bytes.try_into().ok()?)))
            }
            2 => Some(BytecodeConst::String(self.read_string()?)),
            3 => Some(BytecodeConst::Bool(self.read_bool()?)),
            4 => Some(BytecodeConst::Nil),
            _ => None,
        }
    }

    fn read_opcode(&mut self) -> Option<OpCode> {
        match self.read_u8()? {
            0 => Some(OpCode::Const(self.read_u16()?)),
            1 => Some(OpCode::Load(self.read_u16()?)),
            2 => Some(OpCode::Store(self.read_u16()?)),
            3 => Some(OpCode::BindGlobal(self.read_u16()?)),
            10 => Some(OpCode::Add),
            11 => Some(OpCode::Sub),
            12 => Some(OpCode::Mul),
            13 => Some(OpCode::Div),
            14 => Some(OpCode::FloorDiv),
            15 => Some(OpCode::Mod),
            16 => Some(OpCode::Eq),
            17 => Some(OpCode::Ne),
            18 => Some(OpCode::Lt),
            19 => Some(OpCode::Gt),
            20 => Some(OpCode::Le),
            21 => Some(OpCode::Ge),
            22 => Some(OpCode::And),
            23 => Some(OpCode::Or),
            24 => Some(OpCode::Not),
            25 => Some(OpCode::Neg),
            30 => Some(OpCode::Call {
                func: self.read_u16()?,
                argc: self.read_u8()?,
            }),
            31 => Some(OpCode::Return),
            32 => Some(OpCode::Jump(self.read_u32()?)),
            33 => Some(OpCode::JumpIfFalse(self.read_u32()?)),
            34 => Some(OpCode::Pop),
            40 => Some(OpCode::MakeArray(self.read_u16()?)),
            41 => {
                let count = self.read_u16()? as usize;
                let mut fields = Vec::with_capacity(count);
                for _ in 0..count {
                    fields.push(self.read_u16()?);
                }
                Some(OpCode::MakeObject(fields))
            }
            42 => Some(OpCode::MakeInstance {
                class: self.read_u16()?,
                fields: self.read_u16()?,
            }),
            43 => Some(OpCode::GetField(self.read_u16()?)),
            44 => Some(OpCode::SetField(self.read_u16()?)),
            45 => Some(OpCode::GetIndex),
            46 => Some(OpCode::SetIndex),
            47 => Some(OpCode::CallMethod {
                field: self.read_u16()?,
                argc: self.read_u8()?,
            }),
            48 => Some(OpCode::CallSuper {
                method: self.read_u16()?,
                argc: self.read_u8()?,
            }),
            50 => Some(OpCode::TryBegin(self.read_u32()?)),
            51 => Some(OpCode::TryEnd(self.read_u32()?)),
            52 => Some(OpCode::Throw),
            53 => Some(OpCode::Halt),
            _ => None,
        }
    }

    fn read_function(&mut self) -> Option<BytecodeFunction> {
        let name = self.read_string()?;
        let param_count = self.read_u32()? as usize;
        let param_slot_count = self.read_u32()? as usize;
        let mut param_slots = Vec::with_capacity(param_slot_count);
        for _ in 0..param_slot_count {
            param_slots.push(self.read_u16()?);
        }
        let local_count = self.read_u16()?;
        let memoize = self.read_bool()?;
        let code_len = self.read_u32()? as usize;
        let mut code = Vec::with_capacity(code_len);
        for _ in 0..code_len {
            code.push(self.read_opcode()?);
        }
        Some(BytecodeFunction {
            name,
            param_count,
            param_slots,
            local_count,
            memoize,
            code,
        })
    }

    fn read_string_list(&mut self) -> Option<Vec<String>> {
        let count = self.read_u32()? as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.read_string()?);
        }
        Some(out)
    }

    fn read_const_list(&mut self) -> Option<Vec<BytecodeConst>> {
        let count = self.read_u32()? as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.read_const()?);
        }
        Some(out)
    }

    fn read_function_list(&mut self) -> Option<Vec<BytecodeFunction>> {
        let count = self.read_u32()? as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.read_function()?);
        }
        Some(out)
    }
}

pub(crate) fn encode(module: &BytecodeModule) -> Vec<u8> {
    let mut w = Writer::new();
    w.buf.extend_from_slice(MAGIC);
    w.write_u8(WIRE_VERSION);
    w.write_u32(module.cache_version);
    w.write_u64(module.builtin_fingerprint);
    w.write_u32(module.slot_count as u32);
    w.write_optional_string(&module.source_path);
    w.write_fast_path(&module.fast_path);
    w.write_u32(module.constants.len() as u32);
    for c in &module.constants {
        w.write_const(c);
    }
    w.write_u32(module.call_targets.len() as u32);
    for name in &module.call_targets {
        w.write_string(name);
    }
    w.write_u32(module.field_names.len() as u32);
    for name in &module.field_names {
        w.write_string(name);
    }
    w.write_u32(module.slot_names.len() as u32);
    for name in &module.slot_names {
        w.write_string(name);
    }
    w.write_u32(module.functions.len() as u32);
    for f in &module.functions {
        w.write_function(f);
    }
    let classes_blob = serde_json::to_vec(&module.classes).unwrap_or_default();
    let traits_blob = serde_json::to_vec(&module.traits).unwrap_or_default();
    w.write_blob(&classes_blob);
    w.write_blob(&traits_blob);
    w.into_bytes()
}

pub(crate) fn decode(data: &[u8]) -> Option<BytecodeModule> {
    if data.len() < MAGIC.len() + 1 || !data.starts_with(MAGIC) {
        return None;
    }
    let mut r = Reader::new(&data[MAGIC.len()..]);
    let wire_version = r.read_u8()?;
    if wire_version != WIRE_VERSION {
        return None;
    }
    let cache_version = r.read_u32()?;
    let builtin_fingerprint = r.read_u64()?;
    let slot_count = r.read_u32()? as usize;
    let source_path = r.read_optional_string()?;
    let fast_path = r.read_fast_path()?;
    let constants = r.read_const_list()?;
    let call_targets = r.read_string_list()?;
    let field_names = r.read_string_list()?;
    let slot_names = r.read_string_list()?;
    let functions = r.read_function_list()?;
    let classes_blob = r.read_blob()?;
    let traits_blob = r.read_blob()?;
    if r.remaining() != 0 {
        return None;
    }
    let classes: Vec<ClassDef> = serde_json::from_slice(&classes_blob).ok()?;
    let traits: Vec<TraitDef> = serde_json::from_slice(&traits_blob).ok()?;
    Some(BytecodeModule {
        functions,
        constants,
        slot_count,
        call_targets,
        fast_path,
        cache_version,
        builtin_fingerprint,
        source_path,
        classes,
        traits,
        field_names,
        slot_names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile_to_bytecode;
    use niao_parser::parse;

    #[test]
    fn binary_wire_has_magic_header() {
        let src = "fn main() { print(1) }";
        let program = parse(src).unwrap();
        let bc = compile_to_bytecode(&program).unwrap();
        let binary = encode(&bc);
        assert!(binary.starts_with(MAGIC));
        assert!(!binary.is_empty());
        // Roundtrip is the real correctness check; wire may be larger than JSON when
        // classes/traits are embedded as JSON blobs inside the binary container.
        let loaded = decode(&binary).unwrap();
        assert_eq!(loaded.functions.len(), bc.functions.len());
    }

    #[test]
    fn binary_roundtrip_matches_compile() {
        let src = "fn main() { print(1) }";
        let program = parse(src).unwrap();
        let bc = compile_to_bytecode(&program).unwrap();
        let loaded = decode(&encode(&bc)).unwrap();
        assert_eq!(loaded.cache_version, crate::BYTECODE_CACHE_VERSION);
        assert_eq!(loaded.functions.len(), bc.functions.len());
        assert_eq!(loaded.functions[0].code, bc.functions[0].code);
        assert_eq!(loaded.call_targets, bc.call_targets);
    }
}
