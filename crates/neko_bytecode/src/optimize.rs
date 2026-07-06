//! IR and bytecode optimization passes.

use crate::{BytecodeConst, BytecodeFunction, OpCode};
use neko_ast::BinOp;
use neko_ir::{IrConst, IrInstr, IrModule};

fn const_as_i64(constants: &[IrConst], idx: usize) -> Option<i64> {
    match constants.get(idx)? {
        IrConst::Int(v) => Some(*v),
        _ => None,
    }
}

fn eval_binop(op: BinOp, a: i64, b: i64) -> Option<i64> {
    match op {
        BinOp::Add => a.checked_add(b),
        BinOp::Sub => a.checked_sub(b),
        BinOp::Mul => a.checked_mul(b),
        BinOp::Div if b != 0 => Some(a / b),
        BinOp::Mod if b != 0 => Some(a % b),
        _ => None,
    }
}

fn try_fold_binary_into(
    constants: &mut Vec<IrConst>,
    code: &[IrInstr],
    i: usize,
) -> Option<(usize, usize)> {
    let IrInstr::Const(ci) = &code[i] else {
        return None;
    };
    let IrInstr::Const(cj) = code.get(i + 1)? else {
        return None;
    };
    let IrInstr::Binary(op) = code.get(i + 2)? else {
        return None;
    };
    let a = const_as_i64(constants, *ci)?;
    let b = const_as_i64(constants, *cj)?;
    let v = eval_binop(*op, a, b)?;
    let n = constants.len();
    constants.push(IrConst::Int(v));
    Some((n, 3))
}

/// Constant-fold integer binary ops on IR (B1) and remove no-op jumps (B2).
pub fn optimize_ir(module: &mut IrModule) {
    for func in &mut module.functions {
        let mut out = Vec::with_capacity(func.instructions.len());
        let mut i = 0;
        while i < func.instructions.len() {
            if let Some((n, skip)) =
                try_fold_binary_into(&mut module.constants, &func.instructions, i)
            {
                out.push(IrInstr::Const(n));
                i += skip;
                continue;
            }
            if let IrInstr::Jump(t) = func.instructions[i] {
                if t == i + 1 {
                    i += 1;
                    continue;
                }
            }
            out.push(func.instructions[i].clone());
            i += 1;
        }
        func.instructions = out;
    }
}

/// Peephole optimizations on lowered bytecode (B3).
pub fn peephole_function(func: &mut BytecodeFunction, constants: &[BytecodeConst]) {
    let mut out = Vec::with_capacity(func.code.len());
    let mut i = 0;
    while i < func.code.len() {
        if i + 2 < func.code.len() {
            if let (
                OpCode::Const(cidx),
                OpCode::Store(slot),
                OpCode::Load(slot2),
            ) = (
                &func.code[i],
                &func.code[i + 1],
                &func.code[i + 2],
            ) {
                if slot == slot2 {
                    out.push(OpCode::Const(*cidx));
                    out.push(OpCode::Store(*slot));
                    i += 3;
                    continue;
                }
            }
        }
        if let OpCode::Jump(t) = func.code[i] {
            if t as usize == i + 1 {
                i += 1;
                continue;
            }
        }
        if i + 1 < func.code.len() {
            if let (OpCode::Const(cidx), OpCode::JumpIfFalse(t)) =
                (&func.code[i], &func.code[i + 1])
            {
                if matches!(constants.get(*cidx as usize), Some(BytecodeConst::Int(0))) {
                    out.push(OpCode::Jump(*t));
                    i += 2;
                    continue;
                }
            }
        }
        out.push(func.code[i].clone());
        i += 1;
    }
    func.code = out;
}

/// Inline callees that are `Load; Return; Halt` with zero params (B6).
pub fn inline_tiny_callees(functions: &mut [BytecodeFunction], _call_targets: &[String]) {
    let mut load_ret: std::collections::HashMap<u16, u16> = std::collections::HashMap::new();
    for (fi, f) in functions.iter().enumerate() {
        if f.param_count != 0 || f.code.len() != 3 {
            continue;
        }
        if let (OpCode::Load(slot), OpCode::Return, OpCode::Halt) =
            (&f.code[0], &f.code[1], &f.code[2])
        {
            load_ret.insert(fi as u16, *slot);
        }
    }
    for f in functions.iter_mut() {
        for op in &mut f.code {
            if let OpCode::Call { func, argc: 0 } = op {
                if let Some(&slot) = load_ret.get(func) {
                    *op = OpCode::Load(slot);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neko_ir::{IrFunction, IrModule};

    #[test]
    fn folds_const_add() {
        let mut m = IrModule {
            functions: vec![IrFunction {
                name: "main".into(),
                params: vec![],
                instructions: vec![
                    IrInstr::Const(0),
                    IrInstr::Const(1),
                    IrInstr::Binary(BinOp::Add),
                ],
            }],
            constants: vec![IrConst::Int(2), IrConst::Int(3)],
            classes: vec![],
            traits: vec![],
            field_names: vec![],
        };
        optimize_ir(&mut m);
        assert_eq!(m.functions[0].instructions.len(), 1);
        assert!(matches!(m.functions[0].instructions[0], IrInstr::Const(2)));
        assert!(matches!(m.constants.get(2), Some(IrConst::Int(5))));
    }
}
