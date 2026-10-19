//! ExtGStateのfill/stroke alphaを、共有resource resolverで解決する。
use crate::{metadata, resource_resolver::ResourceResolver, text_draws::Operand};

/// gsが明示的に更新するalpha。省略された成分は直前の値を維持する。
#[derive(Clone, Copy, Default)]
pub(crate) struct Parameters {
    /// 非stroke描画のalpha。
    fill: Option<f32>,
    /// stroke描画のalpha。
    stroke: Option<f32>,
}

/// q/QとForm呼び出しで継承する現在のalpha。
#[derive(Clone, Copy)]
pub(crate) struct State {
    /// 非stroke描画のalpha。
    pub(crate) fill: f32,
    /// stroke描画のalpha。
    pub(crate) stroke: f32,
}

impl Default for State {
    /// ページ開始時の不透明な描画状態。
    fn default() -> Self {
        Self {
            fill: 1.,
            stroke: 1.,
        }
    }
}

impl State {
    /// 有効なgsだけを適用し、省略された成分や不正なoperandは変更しない。
    pub(crate) fn update(
        &mut self,
        word: &[u8],
        args: &[Operand<'_>],
        mut resolve: impl FnMut(&[u8]) -> Parameters,
    ) {
        if let (b"gs", [Operand::Name(name)]) = (word, args) {
            let parameters = resolve(name);
            if let Some(fill) = parameters.fill {
                self.fill = fill;
            }
            if let Some(stroke) = parameters.stroke {
                self.stroke = stroke;
            }
        }
    }
}

/// 現在のResourcesだけで名前を解決し、間接参照とescaped nameにも対応する。
pub(crate) fn resolve<'a>(
    resources: &'a [u8],
    name: &[u8],
    resolver: &mut ResourceResolver<'_, 'a>,
) -> Parameters {
    let dictionary = metadata::value(resources, b"ExtGState")
        .and_then(|value| resolver.resolve(value))
        .and_then(|(states, _)| {
            metadata::entries(states)
                .into_iter()
                .flatten()
                .find_map(|(key, value)| {
                    crate::metadata_names::same_resource_name(key, name).then_some(value)
                })
        })
        .and_then(|value| resolver.resolve(value));
    let Some((dictionary, _)) = dictionary else {
        return Parameters::default();
    };
    let mut alpha = |key| {
        metadata::value(dictionary, key)
            .and_then(|value| resolver.resolve(value))
            .and_then(|(value, _)| crate::page_attributes::number_value(value))
            .filter(|value| (0. ..=1.).contains(value))
    };
    Parameters {
        fill: alpha(b"ca"),
        stroke: alpha(b"CA"),
    }
}

/// gsを含まない通常本文では追加のresource索引を作らない。
pub(crate) fn is_used(content: &[u8]) -> bool {
    content.windows(2).any(|bytes| bytes == b"gs")
}
