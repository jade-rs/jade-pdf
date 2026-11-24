//! 通常のForm状態を継承し、未実装のグループ合成を画像alphaに置き換えない。
use crate::{metadata, parser::image_draws::paint::Paint, resource_resolver::ResourceResolver};

/// 不透明な通常合成だけを分解し、それ以外は既存の画像代替診断へ渡す。
pub(crate) fn enter<'a>(
    dictionary: &'a [u8],
    mut paint: Paint,
    resolver: &mut ResourceResolver<'_, 'a>,
) -> Paint {
    let supported = metadata::value(dictionary, b"OC").is_none()
        && metadata::value(dictionary, b"Ref").is_none()
        && metadata::value(dictionary, b"Group").is_none_or(|value| {
            let Some((group, _)) = resolver.resolve(value) else {
                return false;
            };
            if crate::page_attributes::is_null(group) {
                return true;
            }
            if !paint.is_opaque() || !metadata::dictionary_entries_complete(group)
            {
                return false;
            }
            let Some(entries) = metadata::entries(group) else {
                return false;
            };
            let mut seen = 0u8;
            for (key, value) in entries {
                let Some(index) = ["Type", "S", "CS", "I", "K"]
                    .iter()
                    .position(|name| crate::metadata_names::resource_name_order(key, name).is_eq())
                else {
                    return false;
                };
                if seen & (1 << index) != 0 {
                    return false;
                }
                seen |= 1 << index;
                let Some((value, _)) = resolver.resolve(value) else {
                    return false;
                };
                let valid = match index {
                    0 => metadata::is_name(value, b"Group"),
                    1 => metadata::is_name(value, b"Transparency"),
                    2 => metadata::is_name(value, b"DeviceRGB"),
                    // 分離・knockoutを伴うグループは描画順全体の合成が必要。
                    3 | 4 => {
                        let mut lexer = crate::syntax::Lexer {
                            bytes: value,
                            cursor: 0,
                        };
                        matches!(lexer.next(), Some(crate::syntax::Token::Word(b"false")))
                            && lexer.next().is_none()
                    }
                    _ => false,
                };
                if !valid {
                    return false;
                }
            }
            seen & (1 << 1) != 0
        });
    if !supported {
        paint.apply_state(None);
    }
    paint
}
