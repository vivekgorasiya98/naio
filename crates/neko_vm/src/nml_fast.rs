//! VM fast paths for hot NML builtins.

use crate::fast_val::FastVal;
use neko_ast::Span;
use neko_runtime::nml;
use neko_runtime::{Value, ValueRef};

#[derive(Clone, Copy)]
pub enum NmlFastPath {
    Matmul = 0,
    Forward = 1,
    BackwardStep = 2,
}

impl NmlFastPath {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "nml_matmul" => Some(Self::Matmul),
            "nml_forward" => Some(Self::Forward),
            "nml_backward_step" => Some(Self::BackwardStep),
            _ => None,
        }
    }

    pub fn try_execute(
        self,
        stack: &[FastVal],
        heap: &[ValueRef],
        argc: usize,
    ) -> Option<u64> {
        if stack.len() < argc {
            return None;
        }
        let base = stack.len() - argc;
        let args = &stack[base..];
        let span = Span::dummy();
        match self {
            NmlFastPath::Matmul if argc == 2 => {
                let a = nml_handle_id(args[0], heap)?;
                let b = nml_handle_id(args[1], heap)?;
                nml::matmul_handles(a, b, span).ok()
            }
            NmlFastPath::Forward if argc == 2 => {
                let model = nml_handle_id(args[0], heap)?;
                let x = nml_handle_id(args[1], heap)?;
                nml::forward_handles(model, x, span).ok()
            }
            _ => None,
        }
    }

    pub fn try_backward_step(stack: &[FastVal], heap: &[ValueRef], argc: usize) -> bool {
        if argc != 3 || stack.len() < argc {
            return false;
        }
        let base = stack.len() - argc;
        let args = &stack[base..];
        let span = Span::dummy();
        let Some(trainer) = nml_handle_id(args[0], heap) else {
            return false;
        };
        let Some(pred) = nml_handle_id(args[1], heap) else {
            return false;
        };
        let Some(y) = nml_handle_id(args[2], heap) else {
            return false;
        };
        nml::backward_step_handles(trainer, pred, y, span).is_ok()
    }
}

fn nml_handle_id(v: FastVal, heap: &[ValueRef]) -> Option<u64> {
    match v {
        FastVal::Heap(idx) => match &*heap[idx as usize].borrow() {
            Value::NmlHandle(id) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}
