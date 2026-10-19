use crate::syntax::{Lexer, Token, integer, name_is};

/// 現在の画像 decoder が処理できる単一 filter。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFilter {
    /// JPEG 画像。
    Dct,
    /// JBIG2 画像。
    Jbig2,
    /// CCITT Fax 画像。
    Ccitt,
    /// zlib 圧縮画素。
    Flate,
}

impl ImageFilter {
    /// metadata や nested dictionary を除外し、単一 filter のみ選択する。
    pub(crate) fn read(dict: &[u8]) -> Option<Self> {
        let mut value = dictionary_value(dict, b"Filter")?;
        let name = match value.next()? {
            Token::Name(name) => name,
            Token::Open(false) => {
                let Some(Token::Name(name)) = value.next() else {
                    return None;
                };
                if !matches!(value.next(), Some(Token::Close(false))) {
                    return None;
                }
                name
            }
            _ => return None,
        };
        [
            (b"DCTDecode".as_slice(), Self::Dct),
            (b"JBIG2Decode", Self::Jbig2),
            (b"CCITTFaxDecode", Self::Ccitt),
            (b"FlateDecode", Self::Flate),
        ]
        .into_iter()
        .find_map(|(expected, filter)| name_is(name, expected).then_some(filter))
    }
}

/// 最外 dictionary の指定 key の値を読む lexer を返す。
fn dictionary_value<'a>(dict: &'a [u8], key: &[u8]) -> Option<Lexer<'a>> {
    let mut lexer = Lexer {
        bytes: dict,
        cursor: 0,
    };
    if !matches!(lexer.next(), Some(Token::Open(true))) {
        return None;
    }
    while let Some(Token::Name(name)) = lexer.next() {
        if name_is(name, key) {
            return Some(lexer);
        }
        skip_value(&mut lexer)?;
    }
    None
}

/// key に続く値を読み飛ばし、name 値や container 内の名前を key と誤認しない。
fn skip_value(lexer: &mut Lexer<'_>) -> Option<()> {
    match lexer.next()? {
        Token::Open(_) => {
            let mut depth = 1usize;
            while depth > 0 {
                match lexer.next()? {
                    Token::Open(_) => depth += 1,
                    Token::Close(_) => depth -= 1,
                    _ => {}
                }
            }
        }
        Token::Word(word) if integer(word).is_some() => {
            let mut reference = lexer.clone();
            if matches!(reference.next(), Some(Token::Word(word)) if integer(word).is_some())
                && matches!(reference.next(), Some(Token::Word(b"R")))
            {
                *lexer = reference;
            }
        }
        Token::Close(_) => return None,
        _ => {}
    }
    Some(())
}
