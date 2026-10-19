/// 正規化せずUnicode文字単位で挿入・削除・置換の最小回数を数える。
pub fn edit_distance(expected: &str, actual: &str) -> usize {
    let expected: Vec<_> = expected.chars().collect();
    let mut row: Vec<_> = (0..=expected.len()).collect();
    for (i, actual_char) in actual.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, expected_char) in expected.iter().enumerate() {
            let previous = row[j + 1];
            row[j + 1] = (diagonal + usize::from(actual_char != *expected_char))
                .min(row[j] + 1)
                .min(previous + 1);
            diagonal = previous;
        }
    }
    row[expected.len()]
}
