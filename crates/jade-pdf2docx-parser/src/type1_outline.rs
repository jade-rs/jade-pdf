//! Type1の外形を、PostScriptを実行せず有限の型付き演算で読む。
#[path = "type1_outline_charstring.rs"]
mod charstring;
#[path = "type1_outline_crypt.rs"]
mod crypt;
#[path = "type1_outline_private.rs"]
mod private;
#[path = "type1_outline_standard.rs"]
mod standard;
#[path = "type1_outline_type2.rs"]
mod type2;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::DecodeBudget;

    /// PDF用の所有FontFileを、その二層の復号と静的辞書から既知の輪郭へつなぐ。
    #[test]
    fn owned_fontfile_keeps_both_glyphs_through_encryption_layers() {
        let (source, length1, length2, _) = crate::type1_embedding_tests::program();
        let budget = DecodeBudget::default();
        let decrypted =
            crypt::decrypt(&budget, &source[length1..length1 + length2], 55665, 4).unwrap();
        let private = private::parse(&budget, &decrypted).unwrap();
        assert!(private.subrs.is_empty());
        assert_eq!(private.glyphs.len(), 2);
        let glyphs = private
            .glyphs
            .iter()
            .map(|entry| {
                let bytes =
                    crypt::decrypt(&budget, entry.bytes, 4330, private.prefix.unwrap()).unwrap();
                charstring::decode(&budget, &bytes, &[]).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(private.glyphs[0].name, b".notdef");
        assert_eq!(glyphs[0].advance, 600.);
        assert!(glyphs[0].commands.is_empty());
        assert_eq!(private.glyphs[1].name, b"A");
        assert_eq!(
            glyphs[1],
            charstring::Glyph {
                advance: 600.,
                commands: vec![
                    charstring::Command::Move([0., 0.]),
                    charstring::Command::Line([300., 700.]),
                    charstring::Command::Line([600., 0.]),
                    charstring::Command::Line([0., 0.]),
                    charstring::Command::Close,
                ],
            }
        );
        assert!(budget.check().is_ok());
    }
}
