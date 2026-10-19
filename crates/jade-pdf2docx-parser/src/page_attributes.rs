//! 有効 object から page tree の表示属性と Resource 継承を解決する。
use crate::{
    ParseError, ParseLimits,
    decode::DecodeBudget,
    limits::check_limit,
    metadata,
    syntax::{Lexer, Token},
};
use std::path::Path;

/// generation を保持した有効 object 本文の索引。
pub(crate) use crate::object_index::Objects;

/// 継承した Resources の辞書を再取得する位置。
#[derive(Clone, Copy)]
pub(crate) enum ResourceLocation {
    /// Page/Pages の Resources に直接入った辞書。
    Inline(u32),
    /// 間接参照を解決した最終 object の辞書。
    Indirect(u32),
}

/// 親から継承する属性。辞書本文は解析中の入力を借用する。
#[derive(Clone, Copy, Default)]
pub(crate) struct Inherited<'a> {
    /// 最も近い非 null の MediaBox。
    media_box: Option<&'a [u8]>,
    /// 最も近い非 null の CropBox。
    crop_box: Option<&'a [u8]>,
    /// 最も近い非 null の Rotate。
    rotation: Option<&'a [u8]>,
    /// 最も近い非 null の Resources と参照位置。
    resources: Option<(&'a [u8], ResourceLocation)>,
}

/// 検証済みの所属 Page と継承属性。
pub(crate) struct Page {
    /// 最新 xref で有効な Page object 番号。
    pub(crate) number: u32,
    /// 有限で正の面積を持つ MediaBox の正規化座標。
    pub(crate) media_box: [f32; 4],
    /// MediaBox と交差させた正の面積を持つ CropBox。
    pub(crate) crop_box: Option<[f32; 4]>,
    /// 90 度の整数倍として検証済みの回転角。
    pub(crate) rotation: i32,
    /// Resources 辞書の参照位置。欠落時は空の resource として扱う。
    pub(crate) resources: Option<ResourceLocation>,
}

impl<'a> Inherited<'a> {
    /// null は欠落として扱い、現在の node の値で親の属性を上書きする。
    pub(crate) fn at_node(
        self,
        path: &Path,
        objects: &Objects<'a>,
        number: u32,
        node: &'a [u8],
        limits: &ParseLimits,
        budget: &DecodeBudget,
    ) -> Result<Self, ParseError> {
        let mut result = self;
        if let Some(value) = metadata::value(node, b"MediaBox") {
            let (value, _) = resolve_value(path, objects, value, limits, budget)?;
            if !is_null(value) {
                result.media_box = Some(value);
            }
        }
        for (name, destination) in [
            (b"CropBox".as_slice(), &mut result.crop_box),
            (b"Rotate".as_slice(), &mut result.rotation),
        ] {
            if let Some(value) = metadata::value(node, name) {
                let (value, _) = resolve_value(path, objects, value, limits, budget)?;
                if !is_null(value) {
                    *destination = Some(value);
                }
            }
        }
        if let Some(value) = metadata::value(node, b"Resources") {
            let (value, indirect) = resolve_value(path, objects, value, limits, budget)?;
            if !is_null(value) {
                let location = indirect
                    .map(ResourceLocation::Indirect)
                    .unwrap_or(ResourceLocation::Inline(number));
                result.resources = Some((value, location));
            }
        }
        Ok(result)
    }

    /// Leaf の寸法・切り抜き・回転と Resource 辞書の型・参照を検査する。
    pub(crate) fn page(
        self,
        path: &Path,
        objects: &Objects<'a>,
        number: u32,
        limits: &ParseLimits,
        budget: &DecodeBudget,
    ) -> Result<Page, ParseError> {
        let value = self
            .media_box
            .ok_or_else(|| invalid(path, "missing inherited MediaBox"))?;
        let bounds = rectangle_value(path, objects, value, limits, budget)?;
        let crop_box = self
            .crop_box
            .map(|value| {
                let crop = rectangle_value(path, objects, value, limits, budget)?;
                crop_intersection(path, bounds, crop)
            })
            .transpose()?;
        let rotation = self
            .rotation
            .map(|value| {
                rotation_value(value)
                    .ok_or_else(|| invalid(path, "Rotate is not an integer multiple of 90"))
            })
            .transpose()?
            .unwrap_or(0);
        if let Some((resources, _)) = self.resources {
            let resources = dictionary_value(path, resources)?;
            crate::page_resources::validate(path, objects, resources, limits, budget)?;
        }
        Ok(Page {
            number,
            media_box: bounds,
            crop_box,
            rotation,
            resources: self.resources.map(|(_, location)| location),
        })
    }
}

/// 間接座標を含む矩形を有限で正の寸法に正規化する。
pub(crate) fn rectangle_value(
    path: &Path,
    objects: &Objects<'_>,
    value: &[u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<[f32; 4], ParseError> {
    let values = crate::metadata_workspace::array_values(budget, value, 4)?
        .filter(|values| values.len() == 4)
        .ok_or_else(|| invalid(path, "page box is not a four-number array"))?;
    let mut bounds = [0.0; 4];
    for (index, value) in values.into_iter().enumerate() {
        let (value, _) = resolve_value(path, objects, value, limits, budget)?;
        bounds[index] = number_value(value)
            .ok_or_else(|| invalid(path, "page box contains an invalid number"))?;
    }
    let bounds = [
        bounds[0].min(bounds[2]),
        bounds[1].min(bounds[3]),
        bounds[0].max(bounds[2]),
        bounds[1].max(bounds[3]),
    ];
    if bounds[2] <= bounds[0]
        || bounds[3] <= bounds[1]
        || !(bounds[2] - bounds[0]).is_finite()
        || !(bounds[3] - bounds[1]).is_finite()
    {
        return Err(invalid(path, "page box has no finite positive area"));
    }
    Ok(bounds)
}

/// PDF 整数字句を精度を落とさず読み、90 度の整数倍だけを許す。
pub(crate) fn rotation_value(value: &[u8]) -> Option<i32> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let Token::Word(word) = lexer.next()? else {
        return None;
    };
    if lexer.next().is_some() {
        return None;
    }
    let number = std::str::from_utf8(word).ok()?.parse::<i32>().ok()?;
    (number % 90 == 0).then_some(number)
}

/// 検査済み矩形の交差を求め、表示面積がない切り抜きを拒否する。
pub(crate) fn crop_intersection(
    path: &Path,
    media: [f32; 4],
    crop: [f32; 4],
) -> Result<[f32; 4], ParseError> {
    let intersection = [
        media[0].max(crop[0]),
        media[1].max(crop[1]),
        media[2].min(crop[2]),
        media[3].min(crop[3]),
    ];
    if intersection[2] <= intersection[0] || intersection[3] <= intersection[1] {
        return Err(invalid(
            path,
            "CropBox has no positive MediaBox intersection",
        ));
    }
    Ok(intersection)
}

/// 間接属性の欠落・世代・循環・参照深度を検査し、最終本文と object 番号を返す。
pub(crate) fn resolve_value<'a>(
    path: &Path,
    objects: &Objects<'a>,
    mut value: &'a [u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<(&'a [u8], Option<u32>), ParseError> {
    let mut visited = crate::metadata_workspace::Set::default();
    let mut number = None;
    while let Some(reference) = metadata::reference_with_generation(value) {
        if !visited.insert(budget, reference)? {
            return Err(invalid(path, "cyclic page attribute reference"));
        }
        check_limit(
            "page attribute depth",
            visited.len() as u64,
            limits.max_container_nesting as u64,
        )?;
        value = objects
            .get(&reference.0)
            .filter(|(generation, _)| *generation == reference.1)
            .map(|(_, body)| *body)
            .ok_or_else(|| invalid(path, "missing or wrong-generation page attribute"))?;
        number = Some(reference.0);
    }
    Ok((value, number))
}

/// 一個だけの最外辞書を検査し、先頭・末尾の偽 token を許さない。
pub(crate) fn dictionary_value<'a>(path: &Path, value: &'a [u8]) -> Result<&'a [u8], ParseError> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    if !matches!(lexer.next(), Some(Token::Open(true))) {
        return Err(invalid(path, "page resource is not a dictionary"));
    }
    let dictionary = metadata::dictionary_iter(value)
        .next()
        .ok_or_else(|| invalid(path, "invalid page resource dictionary"))?;
    let end = dictionary.as_ptr() as usize - value.as_ptr() as usize + dictionary.len();
    if (Lexer {
        bytes: &value[end..],
        cursor: 0,
    })
    .next()
    .is_some()
    {
        return Err(invalid(path, "trailing page resource tokens"));
    }
    Ok(dictionary)
}

/// 一個の null token であるか判定する。
pub(crate) fn is_null(value: &[u8]) -> bool {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    matches!(lexer.next(), Some(Token::Word(b"null"))) && lexer.next().is_none()
}

/// PDF の整数/実数の字句だけを有限 f32 に変換する。
pub(crate) fn number_value(value: &[u8]) -> Option<f32> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let Token::Word(word) = lexer.next()? else {
        return None;
    };
    if lexer.next().is_some()
        || !word
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'+' | b'-' | b'.'))
    {
        return None;
    }
    let number = std::str::from_utf8(word).ok()?.parse::<f32>().ok()?;
    number.is_finite().then_some(number)
}

/// 検証済み Resource の位置から、payload を複製せず辞書を読む。
pub(crate) fn resource_dictionary<'a>(
    objects: &Objects<'a>,
    location: ResourceLocation,
) -> Option<&'a [u8]> {
    let (number, inline) = match location {
        ResourceLocation::Inline(number) => (number, true),
        ResourceLocation::Indirect(number) => (number, false),
    };
    let (_, body) = objects.get(&number)?;
    let dictionary = metadata::dictionary_iter(body).next()?;
    if inline {
        metadata::value(dictionary, b"Resources")
    } else {
        Some(dictionary)
    }
}

/// ページ属性の破損を型付き構造エラーにする。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.into(),
        reason,
    }
}
