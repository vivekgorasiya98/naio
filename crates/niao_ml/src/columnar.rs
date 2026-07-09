//! Columnar epoch training bridge.

use niao_data::ColumnarEpoch;
use crate::trainer::Trainer;
use niao_tensor::TensorResult;

impl Trainer {
    pub fn train_columnar_epoch(&mut self, epoch: &mut ColumnarEpoch) -> TensorResult<f32> {
        epoch.reset();
        let mut total = 0.0f32;
        let mut batches = 0usize;
        while let Some((x, y)) = epoch.next_batch() {
            total += self.train_batch(&x, &y)?;
            batches += 1;
        }
        Ok(if batches > 0 { total / batches as f32 } else { 0.0 })
    }
}
