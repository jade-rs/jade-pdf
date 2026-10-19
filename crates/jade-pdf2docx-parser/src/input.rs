//! 入力サイズと解析キャンセルをchunkごとに確認するfile読込。
use crate::{ParseError, ParseLimits, decode::DecodeBudget, limits::check_limit};
use std::io::{ErrorKind, Read};

/// 最大入力長+1 byteだけ読み、停止した読込の部分bufferを返さない。
pub(crate) fn read(
    mut reader: impl Read,
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<Vec<u8>, ParseError> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        budget.check()?;
        let remaining = limits.max_input_bytes.saturating_sub(bytes.len() as u64);
        let read_limit = remaining.saturating_add(1).min(chunk.len() as u64) as usize;
        let result = reader.read(&mut chunk[..read_limit]);
        budget.check()?;
        let count = match result {
            Ok(count) => count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        if count == 0 {
            return Ok(bytes);
        }
        check_limit(
            "input bytes",
            bytes.len() as u64 + count as u64,
            limits.max_input_bytes,
        )?;
        let required = bytes.len().checked_add(count).ok_or_else(|| {
            std::io::Error::new(ErrorKind::OutOfMemory, "input capacity overflow")
        })?;
        if required > bytes.capacity() {
            let capacity = bytes
                .capacity()
                .saturating_mul(2)
                .max(required)
                .min(usize::try_from(limits.max_input_bytes).unwrap_or(usize::MAX));
            let fresh = budget.try_vector_capacity::<u8>(capacity);
            budget.check()?;
            let mut fresh = fresh.ok_or_else(|| {
                std::io::Error::new(ErrorKind::OutOfMemory, "input allocation stopped")
            })?;
            fresh.append(&mut bytes);
            bytes = fresh;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "input_allocation_tests.rs"]
mod allocation_tests;
