//! Adobe標準OtherSubrsの完全な字句指紋だけを承認し、PostScriptを実行しない。
use crate::decode::DecodeBudget;
use crate::syntax::Token;
#[path = "type1_program_lexer.rs"]
mod lex;
use lex::{Item, Lexer};

/// 完全な定義を確認した証拠。呼出番号だけでは作らない。
#[derive(Clone, Copy)]
pub(super) struct Verified {
    /// 標準Flex定義も含むか。ヒント切替だけの定義とは区別する。
    flex: bool,
}
impl Verified {
    /// 標準Flex定義が含まれる場合だけ曲線の専用処理を許す。
    pub(super) fn allows_flex(self) -> bool {
        self.flex
    }

    /// 型付き演算の所有テストだけで使う、font由来でない承認値。
    #[cfg(test)]
    pub(super) fn for_owned_test() -> Self {
        Self { flex: true }
    }
}

/// 配列全体を字句単位で照合し、末尾位置と承認値を返す。
pub(super) fn verify(budget: &DecodeBudget, bytes: &[u8]) -> Option<(Verified, usize)> {
    let mut lexer = Lexer {
        bytes: &bytes[..bytes.len().min(16_385)],
        cursor: 0,
    };
    if !matches!(lexer.next(budget), Some(Item::Pdf(Token::Open(false)))) {
        return None;
    }
    let mut hash = hmac_sha256::Hash::new();
    hash.update(b"[");
    let mut stack = [0_u8; 32];
    stack[0] = b']';
    let mut depth = 1;
    for _ in 0..8192 {
        let item = lexer.next(budget)?;
        if lexer.cursor > 16_384 {
            return None;
        }
        match item {
            Item::Pdf(Token::Name(body)) => token_hash(&mut hash, b'N', body),
            Item::Pdf(Token::Word(body)) => token_hash(&mut hash, b'W', body),
            Item::Procedure(true) | Item::Pdf(Token::Open(false)) => {
                let (tag, end) = if matches!(item, Item::Procedure(true)) {
                    (b'{', b'}')
                } else {
                    (b'[', b']')
                };
                *stack.get_mut(depth)? = end;
                depth += 1;
                hash.update([tag]);
            }
            Item::Procedure(false) | Item::Pdf(Token::Close(false)) => {
                let end = if matches!(item, Item::Procedure(false)) {
                    b'}'
                } else {
                    b']'
                };
                depth = depth.checked_sub(1)?;
                if stack[depth] != end {
                    return None;
                }
                hash.update([end]);
                if depth == 0 {
                    let fingerprint = hash.finalize();
                    let flex = if fingerprint
                        == [
                            18, 78, 103, 179, 240, 5, 252, 139, 163, 167, 170, 67, 177, 175, 30,
                            77, 126, 59, 182, 180, 245, 249, 6, 128, 138, 246, 184, 87, 55, 74,
                            107, 122,
                        ] {
                        true
                    } else if fingerprint
                        == [
                            10, 244, 123, 55, 204, 30, 44, 18, 11, 188, 5, 168, 95, 183, 170, 189,
                            45, 196, 255, 5, 10, 74, 106, 61, 216, 243, 168, 124, 134, 132, 177,
                            68,
                        ]
                    {
                        false
                    } else {
                        return None;
                    };
                    return (!budget.is_stopped()).then_some((Verified { flex }, lexer.cursor));
                }
            }
            _ => return None,
        }
    }
    None
}

/// nameとwordの境界も長さで区切り、異なる字句列が連結で一致することを防ぐ。
fn token_hash(hash: &mut hmac_sha256::Hash, tag: u8, body: &[u8]) {
    hash.update([tag]);
    hash.update((body.len() as u32).to_be_bytes());
    hash.update(body);
}
