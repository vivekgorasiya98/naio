use niao_ast::{BinOp, Span, UnaryOp};
use niao_runtime::{apply_binop, apply_unaryop, RuntimeError, Value, ValueRef};
use num_bigint::BigInt;
use std::rc::Rc;

/// Arena allocation hook used by the VM heap GC.
pub(crate) trait HeapAlloc {
    fn push_heap(&mut self, value: ValueRef) -> u32;
    fn heap(&self) -> &[ValueRef];
    fn heap_mut(&mut self) -> &mut Vec<ValueRef>;
}

/// Mutable heap view for opcode dispatch (disjoint from `native_refs` borrow).
pub(crate) struct HeapMut<'a> {
    pub vm: &'a mut super::Vm,
}

impl HeapAlloc for HeapMut<'_> {
    fn push_heap(&mut self, value: ValueRef) -> u32 {
        self.vm.alloc_heap(value)
    }

    fn heap(&self) -> &[ValueRef] {
        &self.vm.heap
    }

    fn heap_mut(&mut self) -> &mut Vec<ValueRef> {
        &mut self.vm.heap
    }
}

/// Copy-friendly stack value. Ints/bools/nil never touch the heap.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FastVal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Nil,
    /// Index into VM `native_store` (Rc<RefCell<NativeDs>>).
    Native(u32),
    Heap(u32),
}

impl FastVal {
    pub const NIL: FastVal = FastVal::Nil;

    #[inline(always)]
    pub fn is_truthy(self) -> bool {
        match self {
            FastVal::Bool(b) => b,
            FastVal::Nil => false,
            FastVal::Int(0) => false,
            FastVal::Float(f) if f == 0.0 => false,
            FastVal::Native(_) | FastVal::Heap(_) => true,
            _ => true,
        }
    }

    #[inline(always)]
    pub fn from_const(c: &niao_bytecode::BytecodeConst, heap: &mut impl HeapAlloc) -> Self {
        match c {
            niao_bytecode::BytecodeConst::Int(v) => FastVal::Int(*v),
            niao_bytecode::BytecodeConst::Float(v) => FastVal::Float(*v),
            niao_bytecode::BytecodeConst::Bool(v) => FastVal::Bool(*v),
            niao_bytecode::BytecodeConst::Nil => FastVal::Nil,
            niao_bytecode::BytecodeConst::String(v) => {
                let idx = heap.push_heap(Value::String(v.clone()).ref_cell());
                FastVal::Heap(idx)
            }
        }
    }

    #[inline(always)]
    pub fn to_value_ref(self, heap: &[ValueRef], native_refs: &[ValueRef]) -> ValueRef {
        match self {
            FastVal::Int(v) => Value::Int(v).ref_cell(),
            FastVal::Float(v) => Value::Float(v).ref_cell(),
            FastVal::Bool(v) => Value::Bool(v).ref_cell(),
            FastVal::Nil => Value::Nil.ref_cell(),
            FastVal::Native(i) => Rc::clone(&native_refs[i as usize]),
            FastVal::Heap(i) => Rc::clone(&heap[i as usize]),
        }
    }

    #[inline(always)]
    pub fn binop(
        self,
        op: BinOp,
        rhs: Self,
        heap: &mut impl HeapAlloc,
        native_refs: &[ValueRef],
    ) -> Result<FastVal, RuntimeError> {
        match (self, rhs) {
            (FastVal::Int(a), FastVal::Int(b)) => match op {
                BinOp::Add => match a.checked_add(b) {
                    Some(v) => Ok(FastVal::Int(v)),
                    None => Ok(push_bigint(heap, BigInt::from(a) + BigInt::from(b))),
                },
                BinOp::Sub => match a.checked_sub(b) {
                    Some(v) => Ok(FastVal::Int(v)),
                    None => Ok(push_bigint(heap, BigInt::from(a) - BigInt::from(b))),
                },
                BinOp::Mul => match a.checked_mul(b) {
                    Some(v) => Ok(FastVal::Int(v)),
                    None => Ok(push_bigint(heap, BigInt::from(a) * BigInt::from(b))),
                },
                BinOp::Div => {
                    if b == 0 {
                        return Err(RuntimeError::DivisionByZero { line: 0, col: 0 });
                    }
                    Ok(FastVal::Float(a as f64 / b as f64))
                }
                BinOp::FloorDiv => {
                    if b == 0 {
                        return Err(RuntimeError::DivisionByZero { line: 0, col: 0 });
                    }
                    Ok(FastVal::Int(a / b))
                }
                BinOp::Mod => {
                    if b == 0 {
                        return Err(RuntimeError::DivisionByZero { line: 0, col: 0 });
                    }
                    Ok(FastVal::Int(a % b))
                }
                BinOp::Eq => Ok(FastVal::Bool(a == b)),
                BinOp::Ne => Ok(FastVal::Bool(a != b)),
                BinOp::Lt => Ok(FastVal::Bool(a < b)),
                BinOp::Gt => Ok(FastVal::Bool(a > b)),
                BinOp::Le => Ok(FastVal::Bool(a <= b)),
                BinOp::Ge => Ok(FastVal::Bool(a >= b)),
                BinOp::And => Ok(FastVal::Bool(a != 0 && b != 0)),
                BinOp::Or => Ok(FastVal::Bool(a != 0 || b != 0)),
            },
            (FastVal::Heap(idx), FastVal::Int(b)) => heap_int_binop(op, idx, b, heap, false),
            (FastVal::Int(a), FastVal::Heap(idx)) => heap_int_binop(op, idx, a, heap, true),
            (FastVal::Float(a), FastVal::Float(b)) => Ok(match op {
                BinOp::Add => FastVal::Float(a + b),
                BinOp::Sub => FastVal::Float(a - b),
                BinOp::Mul => FastVal::Float(a * b),
                BinOp::Div => {
                    if b == 0.0 {
                        return Err(RuntimeError::DivisionByZero { line: 0, col: 0 });
                    }
                    FastVal::Float(a / b)
                }
                BinOp::FloorDiv => {
                    if b == 0.0 {
                        return Err(RuntimeError::DivisionByZero { line: 0, col: 0 });
                    }
                    FastVal::Float((a / b).trunc())
                }
                BinOp::Mod => FastVal::Float(a % b),
                BinOp::Eq => FastVal::Bool((a - b).abs() < f64::EPSILON),
                BinOp::Ne => FastVal::Bool((a - b).abs() >= f64::EPSILON),
                BinOp::Lt => FastVal::Bool(a < b),
                BinOp::Gt => FastVal::Bool(a > b),
                BinOp::Le => FastVal::Bool(a <= b),
                BinOp::Ge => FastVal::Bool(a >= b),
                BinOp::And => FastVal::Bool(a != 0.0 && b != 0.0),
                BinOp::Or => FastVal::Bool(a != 0.0 || b != 0.0),
            }),
            (FastVal::Native(_), _) | (_, FastVal::Native(_)) | (FastVal::Heap(_), _) | (_, FastVal::Heap(_)) => {
                let l = self.to_value_ref(heap.heap(), native_refs);
                let r = rhs.to_value_ref(heap.heap(), native_refs);
                let out = apply_binop(op, &l.borrow(), &r.borrow(), Span::dummy())?;
                Ok(value_to_fast(&out, heap))
            }
            _ => {
                let l = self.to_value_ref(heap.heap(), native_refs);
                let r = rhs.to_value_ref(heap.heap(), native_refs);
                let out = apply_binop(op, &l.borrow(), &r.borrow(), Span::dummy())?;
                Ok(value_to_fast(&out, heap))
            }
        }
    }

    #[inline(always)]
    pub fn unaryop(
        self,
        op: UnaryOp,
        heap: &mut impl HeapAlloc,
        native_refs: &[ValueRef],
    ) -> Result<FastVal, RuntimeError> {
        match (self, op) {
            (FastVal::Int(v), UnaryOp::Neg) => Ok(FastVal::Int(-v)),
            (FastVal::Float(v), UnaryOp::Neg) => Ok(FastVal::Float(-v)),
            (FastVal::Bool(b), UnaryOp::Not) => Ok(FastVal::Bool(!b)),
            (FastVal::Int(v), UnaryOp::Not) => Ok(FastVal::Bool(v != 0)),
            (FastVal::Nil, UnaryOp::Not) => Ok(FastVal::Bool(true)),
            _ => {
                let v = self.to_value_ref(heap.heap(), native_refs);
                let out = apply_unaryop(op, &v.borrow(), Span::dummy())?;
                Ok(value_to_fast(&out, heap))
            }
        }
    }
}

#[inline(always)]
fn push_bigint(heap: &mut impl HeapAlloc, value: BigInt) -> FastVal {
    let idx = heap.push_heap(Value::BigInt(value).ref_cell());
    FastVal::Heap(idx)
}

/// In-place bigint op with a small int factor (factorial hot path).
#[inline(always)]
fn heap_int_binop(
    op: BinOp,
    idx: u32,
    n: i64,
    heap: &mut impl HeapAlloc,
    int_on_left: bool,
) -> Result<FastVal, RuntimeError> {
    let cell = &heap.heap()[idx as usize];
    let mut val = cell.borrow_mut();
    if let Value::BigInt(a) = &mut *val {
        match op {
            BinOp::Mul => {
                *a *= n;
                return Ok(FastVal::Heap(idx));
            }
            BinOp::Add => {
                *a += n;
                return Ok(FastVal::Heap(idx));
            }
            BinOp::Sub if !int_on_left => {
                *a -= n;
                return Ok(FastVal::Heap(idx));
            }
            _ => {}
        }
    }
    drop(val);
    let left = if int_on_left {
        Value::Int(n).ref_cell()
    } else {
        Rc::clone(cell)
    };
    let right = if int_on_left {
        Rc::clone(cell)
    } else {
        Value::Int(n).ref_cell()
    };
    let out = apply_binop(op, &left.borrow(), &right.borrow(), Span::dummy())?;
    Ok(update_heap_value(idx, out, heap))
}

#[inline(always)]
fn update_heap_value(idx: u32, out: Value, heap: &mut impl HeapAlloc) -> FastVal {
    match out {
        Value::Int(v) => FastVal::Int(v),
        Value::Float(v) => FastVal::Float(v),
        Value::Bool(v) => FastVal::Bool(v),
        Value::Nil => FastVal::Nil,
        other => {
            *heap.heap_mut()[idx as usize].borrow_mut() = other;
            FastVal::Heap(idx)
        }
    }
}

pub(crate) fn value_to_fast(val: &Value, heap: &mut impl HeapAlloc) -> FastVal {
    match val {
        Value::Int(v) => FastVal::Int(*v),
        Value::Float(v) => FastVal::Float(*v),
        Value::Bool(v) => FastVal::Bool(*v),
        Value::Nil => FastVal::Nil,
        other => {
            let idx = heap.push_heap(match other {
                Value::String(s) => Value::String(s.clone()).ref_cell(),
                Value::BigInt(v) => Value::BigInt(v.clone()).ref_cell(),
                Value::IntArray(items) => Value::IntArray(items.clone()).ref_cell(),
                Value::FloatArray(items) => Value::FloatArray(items.clone()).ref_cell(),
                Value::BoolArray(items) => Value::BoolArray(items.clone()).ref_cell(),
                Value::ByteArray(items) => Value::ByteArray(items.clone()).ref_cell(),
                Value::StringArray(items) => Value::StringArray(items.clone()).ref_cell(),
                Value::Array(items) => Value::Array(items.clone()).ref_cell(),
                Value::Object(map) => Value::Object(map.clone()).ref_cell(),
                Value::Function(f) => Value::Function(f.clone()).ref_cell(),
                Value::NativeFunction(n) => Value::NativeFunction(Rc::clone(n)).ref_cell(),
                Value::Native(ds) => Value::Native(Rc::clone(ds)).ref_cell(),
                _ => Value::Nil.ref_cell(),
            });
            FastVal::Heap(idx)
        }
    }
}
