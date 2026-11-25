//! 明示的な広いTJ間隔に置かれた短い数値を、別々の原本矩形として保持する。

/// 正方向へ一em以上離れた二つの数値だけを、同一TJ内の自動結合から外す。
pub(crate) fn separates(left: &str, right: &str, gap: f32, size: f32) -> bool {
    gap.is_finite()
        && size.is_finite()
        && size > 0.
        && gap >= size
        && decimal(left)
        && decimal(right)
}

/// 短い十進数・符号・三桁区切りだけを認め、散文やCJKのtrackingを列と推測しない。
fn decimal(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.len() > 32 {
        return false;
    }
    let value = value.strip_prefix(['+', '-', '−']).unwrap_or(value);
    let integer = if let Some((integer, fraction)) = value.split_once('.') {
        if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        integer
    } else {
        value
    };
    let mut groups = integer.split(',');
    let first = groups.next().unwrap_or("");
    if first.is_empty() || !first.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if integer.contains(',') && first.len() > 3 {
        return false;
    }
    groups.all(|g| g.len() == 3 && g.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 境界の正負・非有限値と、非数値や不正な区切りを新しい矩形の根拠にしない。
    #[test]
    fn boundary_and_non_decimal_inputs_preserve_legacy_route() {
        assert!(separates("376", "-2.4", 10., 10.));
        assert!(separates("1,234.50", "−2.4", 10., 10.));
        for (gap, size) in [
            (9.99, 10.),
            (-20., 10.),
            (f32::NAN, 10.),
            (f32::INFINITY, 10.),
            (10., 0.),
            (10., f32::NAN),
        ] {
            assert!(!separates("376", "-2.4", gap, size));
        }
        for value in [
            "", "NaN", "1e3", "1.2.3", "12,34", "1234,567", "漢字", "1 2", "--1", ".", "1.",
        ] {
            assert!(!separates(value, "2", 20., 10.));
            assert!(!separates("2", value, 20., 10.));
        }
        assert!(!separates(&"1".repeat(33), "2", 20., 10.));
    }
}
