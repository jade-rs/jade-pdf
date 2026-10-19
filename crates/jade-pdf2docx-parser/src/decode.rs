//! 解析ごとに共有する展開予算・画像画素上限・失敗状態・検査済み stream 境界。
use crate::{ParseError, ParseLimits};
use flate2::read::ZlibDecoder;
use std::{
    cell::{Cell, Ref, RefCell},
    io::Read,
};

#[path = "decode_allocation.rs"]
mod allocation;

/// Option を返す既存 extractor に、上限超過と検査済み境界を共有する解析ローカルの状態。
pub(crate) struct DecodeBudget {
    /// 解析中の既存停止点で検査する、clone間で共有した停止要求。
    cancellation: Option<crate::ParseCancellation>,
    /// 実効バージョンがPDF 2.0以降かを抽出・診断へ共有する。
    pdf20: Cell<bool>,
    /// 正常に復号を終えたCCITT画像・maskで補完した破損行の累積数。
    ccitt_repaired_rows: Cell<u64>,
    /// 共通fallible確保で要求できる累積容量。
    allocation_limit: u64,
    /// 解放・再確保も含む用途横断の予約済み容量。
    allocation_total: Cell<u64>,
    /// 1 stream の展開上限。
    per_stream: u64,
    /// 解析全体の累積展開上限。
    total_limit: u64,
    /// Font 幅表 payload の解析全体での累積上限。
    font_metrics_limit: u64,
    /// cache の新規幅表と直接辞書の幅表へ予約したバイト数。
    font_metrics_total: Cell<u64>,
    /// Resource/Font cache の累積要求容量上限。
    resource_workspace_limit: u64,
    /// 参照解決と Font 共有本体へ予約した容量。
    resource_workspace_total: Cell<u64>,
    /// metadata 索引・検証・抽出配列の累積要求容量上限。
    metadata_workspace_limit: u64,
    /// 再作成と重複除去前の entry を含む予約済み容量。
    metadata_workspace_total: Cell<u64>,
    /// 埋め込み Font 保存・修復配列の累積要求容量上限。
    embedded_font_limit: u64,
    /// 解放・再確保を含む埋め込み Font の予約済み容量。
    embedded_font_total: Cell<u64>,
    /// CID 幅表の一時配列を累積で制限する予算。
    font_width_workspace_limit: u64,
    /// 既に予約した assignment/successor 配列の累積バイト数。
    font_width_workspace_total: Cell<u64>,
    /// ToUnicode の範囲展開・entry 走査へ許可する累積処理数。
    tounicode_limit: u64,
    /// 再解析と無効値を含む予約済みの処理数。
    tounicode_total: Cell<u64>,
    /// ToUnicode mapping の key/value payload の累積上限。
    tounicode_bytes_limit: u64,
    /// 上書きと再解析を含む予約済み mapping payload。
    tounicode_bytes_total: Cell<u64>,
    /// ToUnicode lookup record と code space 配列の累積要求容量上限。
    tounicode_storage_limit: u64,
    /// 再確保・再解析を含む予約済み配列容量。
    tounicode_storage_total: Cell<u64>,
    /// CMap 選択・本文の入力サイズと UTF-8 置換容量の累積上限。
    tounicode_input_limit: u64,
    /// 再走査と置換 buffer を含む予約済み byte 数。
    tounicode_input_total: Cell<u64>,
    /// Form呼び出し・q/QとRootなしページ親継承の最大深度。
    container_depth_limit: usize,
    /// 画像 buffer を確保する前に検査する最大画素数。
    image_pixel_limit: u64,
    /// PNG保存・符号化・変形配列の累積上限。
    image_storage_limit: u64,
    /// 解放と再確保も含む画像配列の予約容量。
    image_storage_total: Cell<u64>,
    /// 同じ解析内で再利用する、予約済みJPEG復号用配列。
    jpeg_pixels: RefCell<Vec<u8>>,
    /// 同じ解析内で再利用する、予約済みJPEGのPNG圧縮用配列。
    jpeg_compressed: RefCell<Vec<u8>>,
    /// これまでに展開した byte 数。再展開も数える。
    decoded_total: Cell<u64>,
    /// 失敗を呼び出し元へ返した後も展開を再開しないための停止状態。
    stopped: Cell<bool>,
    /// エラー取得後もキャンセル理由を保持する。
    cancelled: Cell<bool>,
    /// 元の入力に対して検査済みの stream 境界。
    streams: RefCell<Option<crate::stream_index::StreamIndex>>,
    /// 構造検査が読んだ最新 xref。境界索引の作成時に移す。
    xref: RefCell<Option<crate::xref::XrefIndex>>,
    /// 最新の圧縮 object 本文を解析全体で共有し、再展開を避ける。
    compressed: RefCell<Option<crate::object_bytes::CompressedObjects>>,
    /// 元入力の完成合成本文を複製せず共有する。再検査で破棄する。
    parser_source: RefCell<Option<std::rc::Rc<Vec<u8>>>>,
    /// 最初の資源超過・allocation失敗・キャンセル。以後の展開を停止する。
    failure: RefCell<Option<ParseError>>,
    /// 検査済み page tree の Page object 番号を Kids 順で共有する。
    pages: RefCell<Option<Vec<crate::page_attributes::Page>>>,
    /// Rootのない互換ページの検証済み表示情報を抽出へ共有する。
    legacy_geometry: RefCell<Option<Vec<crate::legacy_page_geometry_validation::Geometry>>>,
    /// 検査済みの元文書に限る完成 CMap の共有索引。
    cmaps: RefCell<crate::tounicode_cache::CMapCache>,
}

impl Default for DecodeBudget {
    /// 通常の解析上限から展開予算を作る。
    fn default() -> Self {
        Self::new(&ParseLimits::default())
    }
}

impl DecodeBudget {
    /// 呼び出し元の解析上限から空の予算を作る。
    pub(crate) fn new(limits: &ParseLimits) -> Self {
        Self {
            cancellation: None,
            pdf20: Cell::new(false),
            ccitt_repaired_rows: Cell::new(0),
            allocation_limit: limits.max_total_allocation_bytes,
            allocation_total: Cell::new(0),
            per_stream: limits.max_decoded_stream_bytes,
            total_limit: limits.max_total_decoded_bytes,
            font_metrics_limit: limits.max_total_font_metrics_bytes,
            font_metrics_total: Cell::new(0),
            resource_workspace_limit: limits.max_total_resource_workspace_bytes,
            resource_workspace_total: Cell::new(0),
            metadata_workspace_limit: limits.max_total_metadata_workspace_bytes,
            metadata_workspace_total: Cell::new(0),
            embedded_font_limit: limits.max_total_embedded_font_bytes,
            embedded_font_total: Cell::new(0),
            font_width_workspace_limit: limits.max_total_font_width_workspace_bytes,
            font_width_workspace_total: Cell::new(0),
            tounicode_limit: limits.max_total_tounicode_entries,
            tounicode_total: Cell::new(0),
            tounicode_bytes_limit: limits.max_total_tounicode_bytes,
            tounicode_bytes_total: Cell::new(0),
            tounicode_storage_limit: limits.max_total_tounicode_storage_bytes,
            tounicode_storage_total: Cell::new(0),
            tounicode_input_limit: limits.max_total_tounicode_input_bytes,
            tounicode_input_total: Cell::new(0),
            container_depth_limit: limits.max_container_nesting,
            image_pixel_limit: limits.max_image_pixels,
            image_storage_limit: limits.max_total_image_storage_bytes,
            image_storage_total: Cell::new(0),
            jpeg_pixels: RefCell::new(Vec::new()),
            jpeg_compressed: RefCell::new(Vec::new()),
            decoded_total: Cell::new(0),
            stopped: Cell::new(false),
            cancelled: Cell::new(false),
            failure: RefCell::new(None),
            compressed: RefCell::new(None),
            parser_source: RefCell::new(None),
            streams: RefCell::new(None),
            xref: RefCell::new(None),
            pages: RefCell::new(None),
            legacy_geometry: RefCell::new(None),
            cmaps: RefCell::new(Default::default()),
        }
    }

    /// 同じ解析予算に任意のキャンセル要求を保持する。
    pub(crate) fn with_cancellation(
        limits: &ParseLimits,
        cancellation: Option<&crate::ParseCancellation>,
    ) -> Self {
        let mut budget = Self::new(limits);
        budget.cancellation = cancellation.cloned();
        budget
    }

    /// 完了したCCITT復号の補完行を、画像object単位の品質報告へ共有する。
    pub(crate) fn record_ccitt_repairs(&self, rows: u32) -> Option<()> {
        self.ccitt_repaired_rows.set(
            self.ccitt_repaired_rows
                .get()
                .checked_add(u64::from(rows))?,
        );
        Some(())
    }

    /// 元画像とそのmaskの復号前後で比較する補完行の累積数を取得する。
    pub(crate) fn ccitt_repaired_rows(&self) -> u64 {
        self.ccitt_repaired_rows.get()
    }

    /// Catalogを含めて解決済みの実効バージョンを保存する。
    pub(crate) fn set_pdf_version(&self, version: &crate::PdfVersion) {
        self.pdf20.set(version.major >= 2);
    }

    /// PDF 2.0以降のType3描画規則を使うかを返す。
    pub(crate) fn is_pdf20(&self) -> bool {
        self.pdf20.get()
    }

    /// 構造検査済みの境界を解析内で共有する。payload は複製しない。
    pub(crate) fn cache_streams(&self, bytes: &[u8], streams: &[crate::structure::StreamSpan<'_>]) {
        self.pdf20.set(false);
        *self.streams.borrow_mut() = None;
        *self.compressed.borrow_mut() = None;
        *self.parser_source.borrow_mut() = None;
        *self.pages.borrow_mut() = None;
        *self.legacy_geometry.borrow_mut() = None;
        *self.cmaps.borrow_mut() = Default::default();
        let Some(mut index) = crate::stream_index::StreamIndex::new(self, bytes, streams) else {
            return;
        };
        if index
            .activate(self, bytes, self.xref.borrow_mut().take())
            .is_none()
        {
            return;
        }
        *self.streams.borrow_mut() = Some(index);
    }

    /// 検査済みの元入力から、停止していない解析にだけ完成 stream map を返す。
    pub(crate) fn cached_cmap(
        &self,
        bytes: &[u8],
        number: u32,
    ) -> Option<crate::text::ToUnicodeCMap> {
        if self.is_stopped() {
            return None;
        }
        self.stream_index(bytes)?;
        self.cmaps.borrow().get(number)
    }

    /// 元文書の完成 stream map を容量予算内で保存する。
    pub(crate) fn cache_cmap(
        &self,
        bytes: &[u8],
        number: u32,
        cmap: crate::text::ToUnicodeCMap,
    ) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        if self.stream_index(bytes).is_some() {
            self.cmaps.borrow_mut().insert(self, number, cmap)?;
        }
        Some(())
    }

    /// 検査済み文書の完成集約を停止していない解析に返す。
    pub(crate) fn cached_global_cmap(&self, bytes: &[u8]) -> Option<crate::text::ToUnicodeCMap> {
        if self.is_stopped() {
            return None;
        }
        self.stream_index(bytes)?;
        self.cmaps.borrow().global.clone()
    }

    /// 元入力の完成集約を共有し、後続の選択走査と統合を省略する。
    pub(crate) fn cache_global_cmap(&self, bytes: &[u8], cmap: crate::text::ToUnicodeCMap) {
        if !self.is_stopped() && self.stream_index(bytes).is_some() {
            self.cmaps.borrow_mut().global = Some(cmap);
        }
    }

    /// 検査済み page tree の順序を保存する。
    pub(crate) fn cache_pages(&self, pages: Option<Vec<crate::page_attributes::Page>>) {
        *self.pages.borrow_mut() = pages;
    }

    /// 互換ページの幾何属性を所有権の移動だけで共有する。
    pub(crate) fn cache_legacy_geometry(
        &self,
        geometry: Vec<crate::legacy_page_geometry_validation::Geometry>,
    ) {
        *self.legacy_geometry.borrow_mut() = Some(geometry);
    }

    /// 正式または互換ページの幾何属性を検証済みか返す。
    pub(crate) fn has_page_geometry(&self) -> bool {
        self.has_page_tree() || self.legacy_geometry.borrow().is_some()
    }

    /// 正式な page tree の所属を検査済みか返す。
    pub(crate) fn has_page_tree(&self) -> bool {
        self.pages.borrow().is_some()
    }

    /// 合成 metadata を所属順に借用し、出力配列を確保前に累積予約する。
    pub(crate) fn page_dictionaries<'a>(&self, source: &'a [u8]) -> Vec<&'a [u8]> {
        if self.is_stopped() {
            return Vec::new();
        }
        let pages = self.pages.borrow();
        let Some(pages) = pages.as_ref() else {
            let count = crate::metadata::page_iter(source).count();
            let Some(mut selected) = self.metadata_vector(count) else {
                return Vec::new();
            };
            selected.extend(crate::metadata::page_iter(source));
            return selected;
        };
        let Some(objects) = crate::object_index::build(self, source) else {
            return Vec::new();
        };
        let dictionaries = || {
            pages
                .iter()
                .filter_map(|page| objects.get(&page.number))
                .filter_map(|(_, body)| crate::metadata::dictionary_iter(body).next())
        };
        let Some(mut selected) = self.metadata_vector(dictionaries().count()) else {
            return Vec::new();
        };
        selected.extend(dictionaries());
        selected
    }

    /// 正式・互換入力の検証済み MediaBox をページ順に返す。
    pub(crate) fn page_media_box(&self, page: usize) -> Option<[f32; 4]> {
        self.pages
            .borrow()
            .as_ref()
            .and_then(|pages| pages.get(page))
            .map(|page| page.media_box)
            .or_else(|| self.legacy_geometry.borrow().as_ref()?.get(page)?.media_box)
    }

    /// 検証済み CropBox を返す。外側の None は未検証入力を表す。
    pub(crate) fn page_crop_box(&self, page: usize) -> Option<Option<[f32; 4]>> {
        self.pages
            .borrow()
            .as_ref()
            .and_then(|pages| pages.get(page))
            .map(|page| page.crop_box)
            .or_else(|| {
                self.legacy_geometry
                    .borrow()
                    .as_ref()?
                    .get(page)
                    .map(|page| page.crop_box)
            })
    }

    /// 正式・互換入力の検証済み回転角をページ順に返す。
    pub(crate) fn page_rotation(&self, page: usize) -> Option<i32> {
        self.pages
            .borrow()
            .as_ref()
            .and_then(|pages| pages.get(page))
            .map(|page| page.rotation)
            .or_else(|| {
                self.legacy_geometry
                    .borrow()
                    .as_ref()?
                    .get(page)
                    .map(|page| page.rotation)
            })
    }

    /// Resource の位置を合成本文へ対応させ、ページ順の配列を累積予約する。
    pub(crate) fn page_resources<'a>(&self, source: &'a [u8]) -> Option<Vec<Option<&'a [u8]>>> {
        let pages = self.pages.borrow();
        let pages = pages.as_ref()?;
        let objects = crate::object_index::build(self, source)?;
        let mut resources = self.metadata_vector(pages.len())?;
        resources.extend(pages.iter().map(|page| {
            page.resources.and_then(|location| {
                crate::page_attributes::resource_dictionary(&objects, location)
            })
        }));
        Some(resources)
    }

    /// 検査済み xref を保存し、extractor 側で再展開せず共有する。
    pub(crate) fn cache_xref(&self, xref: Option<crate::xref::XrefIndex>) {
        *self.xref.borrow_mut() = xref;
    }

    /// 元入力と同じ slice にだけ境界を返す。展開後の合成入力には使わない。
    pub(crate) fn stream_index(
        &self,
        bytes: &[u8],
    ) -> Option<Ref<'_, crate::stream_index::StreamIndex>> {
        Ref::filter_map(self.streams.borrow(), |index| {
            index.as_ref().filter(|index| index.matches(bytes))
        })
        .ok()
    }

    /// 検査済みの元入力に限り、圧縮 object の共有キャッシュを返す。
    pub(crate) fn compressed_objects(
        &self,
        bytes: &[u8],
    ) -> Option<crate::object_bytes::CompressedObjects> {
        if self.is_stopped() {
            return None;
        }
        self.stream_index(bytes)?;
        self.compressed.borrow().clone()
    }

    /// 元入力の有効な圧縮本文を保存する。合成入力の結果は保存しない。
    pub(crate) fn cache_compressed_objects(
        &self,
        bytes: &[u8],
        objects: crate::object_bytes::CompressedObjects,
    ) {
        if !self.is_stopped() && self.stream_index(bytes).is_some() {
            *self.parser_source.borrow_mut() = None;
            *self.compressed.borrow_mut() = Some(objects);
        }
    }

    /// 元入力の xref と間接参照の世代を照合する。
    pub(crate) fn reference(&self, bytes: &[u8], value: &[u8]) -> Option<u32> {
        match self.stream_index(bytes) {
            Some(index) => index.reference(value),
            None => crate::metadata::reference(value),
        }
    }

    /// 停止前かつ検査済み元入力に限り、完成合成本文を共有して返す。
    pub(crate) fn parser_source(&self, bytes: &[u8]) -> Option<std::rc::Rc<Vec<u8>>> {
        if self.is_stopped() {
            return None;
        }
        self.stream_index(bytes)?;
        self.parser_source.borrow().clone()
    }

    /// 予約済みの完成合成本文だけを検査済み元入力のcacheへ保存する。
    pub(crate) fn cache_parser_source(&self, bytes: &[u8], source: std::rc::Rc<Vec<u8>>) {
        if !self.is_stopped() && self.stream_index(bytes).is_some() {
            *self.parser_source.borrow_mut() = Some(source);
        }
    }

    /// 元入力から stream 本文と metadata 内のコメント・文字列を除く。
    /// 展開済み CMap など、元入力以外の slice はそのまま借用する。
    pub(crate) fn structural_bytes<'a>(&self, bytes: &'a [u8]) -> std::borrow::Cow<'a, [u8]> {
        if self.is_stopped() {
            return std::borrow::Cow::Borrowed(&[]);
        }
        match self.stream_index(bytes) {
            Some(index) => {
                let mut source = index.without_payloads(self, bytes);
                crate::metadata::mask_ignored(&mut source);
                std::borrow::Cow::Owned(source)
            }
            None => std::borrow::Cow::Borrowed(bytes),
        }
    }

    /// 内部 extractor が保持した失敗を公開 API に伝える。
    pub(crate) fn check(&self) -> Result<(), ParseError> {
        self.is_stopped();
        match self.failure.borrow_mut().take() {
            Some(error) => Err(error),
            None if self.cancelled.get() => Err(ParseError::Cancelled),
            None => Ok(()),
        }
    }

    /// 最初の上限超過を保存する。
    fn exceeded(&self, resource: &'static str, limit: u64, actual: u64) {
        if self.stopped.replace(true) {
            return;
        }
        *self.failure.borrow_mut() = Some(ParseError::ResourceLimit {
            resource,
            limit,
            actual,
        });
    }

    /// 共有キャンセルを検出し、停止後のplaceholder生成など不要な処理を避ける。
    pub(crate) fn is_stopped(&self) -> bool {
        if !self.stopped.get()
            && self
                .cancellation
                .as_ref()
                .is_some_and(|token| token.is_cancelled())
        {
            self.stopped.set(true);
            self.cancelled.set(true);
            *self.failure.borrow_mut() = Some(ParseError::Cancelled);
        }
        self.stopped.get()
    }

    /// ToUnicode のループ開始前に処理数を累積で予約する。
    pub(crate) fn charge_tounicode_entries(&self, count: u64) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let actual = self.tounicode_total.get().saturating_add(count);
        if actual > self.tounicode_limit {
            self.exceeded("ToUnicode entries", self.tounicode_limit, actual);
            return None;
        }
        self.tounicode_total.set(actual);
        Some(())
    }

    /// 不正 CMap を最初の失敗として保持し、後続の解析を停止する。
    pub(crate) fn invalid_tounicode(&self, reason: &'static str) {
        if !self.is_stopped() {
            self.stopped.set(true);
            *self.failure.borrow_mut() = Some(ParseError::InvalidToUnicode { reason });
        }
    }

    /// mapping の key/value を確保する前に payload byte 数を累積予約する。
    pub(crate) fn charge_tounicode_bytes(&self, length: u64) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let actual = self.tounicode_bytes_total.get().saturating_add(length);
        if actual > self.tounicode_bytes_limit {
            self.exceeded(
                "ToUnicode mapping bytes",
                self.tounicode_bytes_limit,
                actual,
            );
            return None;
        }
        self.tounicode_bytes_total.set(actual);
        Some(())
    }

    /// lookup record と code space 配列の新容量を確保前に累積予約する。
    pub(crate) fn charge_tounicode_storage_bytes(&self, length: u64) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let actual = self.tounicode_storage_total.get().saturating_add(length);
        if actual > self.tounicode_storage_limit {
            self.exceeded(
                "ToUnicode storage bytes",
                self.tounicode_storage_limit,
                actual,
            );
            return None;
        }
        self.tounicode_storage_total.set(actual);
        Some(())
    }

    /// CMap の走査開始前または UTF-8 置換 buffer の確保前に byte 数を予約する。
    pub(crate) fn charge_tounicode_input_bytes(&self, length: u64) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let actual = self.tounicode_input_total.get().saturating_add(length);
        if actual > self.tounicode_input_limit {
            self.exceeded("ToUnicode input bytes", self.tounicode_input_limit, actual);
            return None;
        }
        self.tounicode_input_total.set(actual);
        Some(())
    }

    /// 呼び出し側で予算を予約済みの String 容量を確保し、失敗を保存する。
    pub(crate) fn try_string_capacity(&self, capacity: usize) -> Option<String> {
        if self.is_stopped() {
            return None;
        }
        self.reserve_allocation::<u8>(capacity)?;
        let mut value = String::new();
        if let Err(error) = value.try_reserve_exact(capacity) {
            self.stopped.set(true);
            *self.failure.borrow_mut() = Some(ParseError::Io(std::io::Error::new(
                std::io::ErrorKind::OutOfMemory,
                error,
            )));
            return None;
        }
        Some(value)
    }

    /// 幅表 payload を確保前に予約し、cache hit では呼び出さない。
    pub(crate) fn charge_font_metrics_bytes(&self, length: usize) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let actual = self.font_metrics_total.get().saturating_add(length as u64);
        if actual > self.font_metrics_limit {
            self.exceeded("font metrics bytes", self.font_metrics_limit, actual);
            return None;
        }
        self.font_metrics_total.set(actual);
        Some(())
    }

    /// Resource と Font cache の確保を事前予約し、解放後も累積へ数える。
    pub(crate) fn charge_resource_workspace_bytes(&self, length: u64) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let actual = self.resource_workspace_total.get().saturating_add(length);
        if actual > self.resource_workspace_limit {
            self.exceeded(
                "resource workspace bytes",
                self.resource_workspace_limit,
                actual,
            );
            return None;
        }
        self.resource_workspace_total.set(actual);
        Some(())
    }

    /// PNG保存・符号化・変形・色変換/SMask/codec出力配列を確保前に予約する。
    pub(crate) fn image_vector<T>(&self, capacity: usize) -> Option<Vec<T>> {
        if self.is_stopped() {
            return None;
        }
        let length = (capacity as u64).saturating_mul(std::mem::size_of::<T>() as u64);
        let actual = self.image_storage_total.get().saturating_add(length);
        if actual > self.image_storage_limit {
            self.exceeded("image storage bytes", self.image_storage_limit, actual);
            return None;
        }
        self.image_storage_total.set(actual);
        self.try_vector_capacity(capacity)
    }

    /// 既存容量内では再確保せず、拡張時だけ全容量を予約してJPEGを処理する。
    pub(crate) fn with_jpeg_pixels<T>(
        &self,
        length: usize,
        process: impl FnOnce(&mut [u8]) -> Option<T>,
    ) -> Option<T> {
        if self.is_stopped() {
            return None;
        }
        let mut pixels = self.jpeg_pixels.borrow_mut();
        if pixels.capacity() < length {
            *pixels = self.image_vector(length)?;
        }
        pixels.resize(length, 0);
        let result = process(&mut pixels);
        pixels.clear();
        result
    }

    /// JPEGの圧縮用配列を借用し、予約済み容量を次の画像でも再利用する。
    pub(crate) fn with_jpeg_compressed<T>(
        &self,
        process: impl FnOnce(&mut Vec<u8>) -> Option<T>,
    ) -> Option<T> {
        if self.is_stopped() {
            return None;
        }
        let mut compressed = self.jpeg_compressed.borrow_mut();
        compressed.clear();
        let result = process(&mut compressed);
        compressed.clear();
        result
    }

    /// 埋め込み Font の保存・修復配列を確保前に予約する。
    pub(crate) fn font_vector<T>(&self, capacity: usize) -> Option<Vec<T>> {
        if self.is_stopped() {
            return None;
        }
        let length = (capacity as u64).saturating_mul(std::mem::size_of::<T>() as u64);
        let actual = self.embedded_font_total.get().saturating_add(length);
        if actual > self.embedded_font_limit {
            self.exceeded("embedded font bytes", self.embedded_font_limit, actual);
            return None;
        }
        self.embedded_font_total.set(actual);
        self.try_vector_capacity(capacity)
    }

    /// metadata 索引・検証・抽出配列を確保前に予約し、解放・再作成も累積へ数える。
    pub(crate) fn metadata_vector<T>(&self, capacity: usize) -> Option<Vec<T>> {
        let length = (capacity as u64).saturating_mul(std::mem::size_of::<T>() as u64);
        self.charge_metadata_workspace_bytes(length)?;
        self.try_vector_capacity(capacity)
    }

    /// 配列以外の共有管理領域も、確保前にmetadataの累積容量へ予約する。
    pub(crate) fn charge_metadata_workspace_bytes(&self, length: u64) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let actual = self.metadata_workspace_total.get().saturating_add(length);
        if actual > self.metadata_workspace_limit {
            self.exceeded(
                "metadata workspace bytes",
                self.metadata_workspace_limit,
                actual,
            );
            return None;
        }
        self.metadata_workspace_total.set(actual);
        Some(())
    }

    /// 幅表の一時配列を確保前にまとめて予約し、解放後も累積へ数える。
    pub(crate) fn charge_font_width_workspace_bytes(&self, length: usize) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let actual = self
            .font_width_workspace_total
            .get()
            .saturating_add(length as u64);
        if actual > self.font_width_workspace_limit {
            self.exceeded(
                "font width workspace bytes",
                self.font_width_workspace_limit,
                actual,
            );
            return None;
        }
        self.font_width_workspace_total.set(actual);
        Some(())
    }

    /// 明示的に予算検査した配列の capacity を確保し、allocation 失敗を保持する。
    pub(crate) fn try_vector_capacity<T>(&self, capacity: usize) -> Option<Vec<T>> {
        if self.is_stopped() {
            return None;
        }
        self.reserve_allocation::<T>(capacity)?;
        let mut values = Vec::new();
        if let Err(error) = values.try_reserve_exact(capacity) {
            self.stopped.set(true);
            *self.failure.borrow_mut() = Some(ParseError::Io(std::io::Error::new(
                std::io::ErrorKind::OutOfMemory,
                error,
            )));
            return None;
        }
        Some(values)
    }

    /// 未圧縮 stream の本文を共通展開予算へ加算し、fallible にコピーする。
    pub(crate) fn copy_stream_bytes(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.charge_form_bytes(data.len())?;
        let mut copied = self.try_vector_capacity(data.len())?;
        copied.extend_from_slice(data);
        Some(copied)
    }

    /// 未圧縮本文の保存と Form 描画ごとの走査量を既存展開予算へ加算する。
    pub(crate) fn charge_form_bytes(&self, length: usize) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        if length as u64 > self.per_stream {
            self.exceeded("decoded stream bytes", self.per_stream, length as u64);
            return None;
        }
        let actual = self.decoded_total.get().saturating_add(length as u64);
        if actual > self.total_limit {
            self.exceeded("total decoded bytes", self.total_limit, actual);
            return None;
        }
        self.decoded_total.set(actual);
        Some(())
    }

    /// 非再帰 walker の stack を入力由来の有限な深度に制限する。
    pub(crate) fn check_form_depth(&self, depth: usize) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        if depth > self.container_depth_limit {
            self.exceeded(
                "Form depth",
                self.container_depth_limit as u64,
                depth as u64,
            );
            return None;
        }
        Some(())
    }

    /// Rootなしページの親継承を訪問slot確保・本文取得より先に制限する。
    pub(crate) fn check_page_inheritance_depth(&self, depth: usize) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        if depth > self.container_depth_limit {
            self.exceeded(
                "page inheritance depth",
                self.container_depth_limit as u64,
                depth as u64,
            );
            return None;
        }
        Some(())
    }

    /// Form walkerと画像Do走査のq/Q保存stackを確保前に検査する。
    pub(crate) fn check_form_graphics_depth(&self, depth: usize) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        if depth > self.container_depth_limit {
            self.exceeded(
                "Form graphics state depth",
                self.container_depth_limit as u64,
                depth as u64,
            );
            return None;
        }
        Some(())
    }

    /// 辞書・codec・描画先の実寸法を、積の overflow なしで検査する。
    pub(crate) fn check_image_pixels(&self, width: u32, height: u32) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let pixels = u64::from(width) * u64::from(height);
        if pixels > self.image_pixel_limit {
            self.exceeded("image pixels", self.image_pixel_limit, pixels);
            return None;
        }
        Some(())
    }
}

/// 展開を固定長 chunk で読み、上限 + 1 byte を検出して Vec 追加前に止める。
///
/// 壊れた zlib の扱いは既存 extractor の Option に委ねるが、資源超過は
/// 予算に保存するため、呼び出し側が None を捨てても解析成功にはならない。
pub(crate) fn decode_zlib(data: &[u8], decode_budget: &DecodeBudget) -> Option<Vec<u8>> {
    decode_zlib_buffer(data, decode_budget, false)
}

/// Font 展開配列の全ての再確保を保存予算にも数える。
pub(crate) fn decode_zlib_font(data: &[u8], decode_budget: &DecodeBudget) -> Option<Vec<u8>> {
    decode_zlib_buffer(data, decode_budget, true)
}

/// 展開量を検査し、Font の場合は明示的な新配列へ容量を予約して移す。
fn decode_zlib_buffer(data: &[u8], decode_budget: &DecodeBudget, font: bool) -> Option<Vec<u8>> {
    if decode_budget.is_stopped() {
        return None;
    }
    decode_buffer(ZlibDecoder::new(data), decode_budget, font)
}

/// 展開readerの容量検査とbuffer保存を共通化する。
fn decode_buffer(
    mut decoder: impl Read,
    decode_budget: &DecodeBudget,
    font: bool,
) -> Option<Vec<u8>> {
    let mut decoded = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if decode_budget.is_stopped() {
            return None;
        }
        let remaining = (decode_budget.per_stream - decoded.len() as u64)
            .min(decode_budget.total_limit - decode_budget.decoded_total.get());
        let read_limit = remaining.saturating_add(1).min(chunk.len() as u64) as usize;
        let result = decoder.read(&mut chunk[..read_limit]);
        if decode_budget.is_stopped() {
            return None;
        }
        let count = result.ok()?;
        if count == 0 {
            return Some(decoded);
        }
        let stream_actual = (decoded.len() as u64).saturating_add(count as u64);
        if stream_actual > decode_budget.per_stream {
            decode_budget.exceeded(
                "decoded stream bytes",
                decode_budget.per_stream,
                stream_actual,
            );
            return None;
        }
        let total_actual = decode_budget
            .decoded_total
            .get()
            .saturating_add(count as u64);
        if total_actual > decode_budget.total_limit {
            decode_budget.exceeded(
                "total decoded bytes",
                decode_budget.total_limit,
                total_actual,
            );
            return None;
        }
        if decoded.len().checked_add(count)? > decoded.capacity() {
            let capacity = decoded
                .capacity()
                .saturating_mul(2)
                .max(decoded.len().checked_add(count)?);
            let mut fresh = if font {
                decode_budget.font_vector(capacity)?
            } else {
                decode_budget.try_vector_capacity(capacity)?
            };
            fresh.append(&mut decoded);
            decoded = fresh;
        }
        decoded.extend_from_slice(&chunk[..count]);
        decode_budget.decoded_total.set(total_actual);
    }
}

#[cfg(test)]
#[path = "font_decode_tests.rs"]
mod font_tests;

#[cfg(test)]
#[path = "decode_failure_tests.rs"]
mod failure_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::io::Write;

    /// テスト用 payload を圧縮する。
    fn compressed(bytes: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    /// 2 stream の累積がちょうど上限なら成功し、次の 1 byte を拒否する。
    #[test]
    fn cumulative_limit_is_inclusive_and_failure_stops_decoding() {
        let budget = DecodeBudget::new(&ParseLimits {
            max_decoded_stream_bytes: 4,
            max_total_decoded_bytes: 8,
            ..ParseLimits::default()
        });
        assert_eq!(
            decode_zlib(&compressed(b"abcd"), &budget),
            Some(b"abcd".to_vec())
        );
        assert_eq!(
            decode_zlib(&compressed(b"efgh"), &budget),
            Some(b"efgh".to_vec())
        );
        assert!(decode_zlib(&compressed(b"i"), &budget).is_none());
        assert!(decode_zlib(&compressed(b""), &budget).is_none());
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "total decoded bytes",
                limit: 8,
                actual: 9
            })
        ));
        assert!(decode_zlib(&compressed(b""), &budget).is_none());
    }

    /// capacity overflow は panic せずエラーとなり、後続の確保も停止する。
    #[test]
    fn vector_capacity_failure_stops_further_allocation() {
        let budget = DecodeBudget::default();
        assert!(budget.try_vector_capacity::<u8>(usize::MAX).is_none());
        assert!(budget.is_stopped());
        assert!(budget.try_vector_capacity::<u8>(1).is_none());
        assert!(matches!(budget.check(), Err(ParseError::Io(error))
            if error.kind() == std::io::ErrorKind::OutOfMemory));
    }

    /// String capacity overflow は型付きエラーとなり、停止後は再確保しない。
    #[test]
    fn string_capacity_failure_stops_further_allocation() {
        let budget = DecodeBudget::default();
        assert!(budget.try_string_capacity(usize::MAX).is_none());
        assert!(budget.is_stopped());
        assert!(budget.try_string_capacity(1).is_none());
        assert!(budget.charge_tounicode_bytes(1).is_none());
        assert!(matches!(budget.check(), Err(ParseError::Io(error))
            if error.kind() == std::io::ErrorKind::OutOfMemory));
    }

    /// workspace の境界を含み、最初の超過を後続予約で上書きしない。
    #[test]
    fn font_width_workspace_reservations_are_cumulative_and_stop() {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_font_width_workspace_bytes: 10,
            ..Default::default()
        });
        assert_eq!(budget.charge_font_width_workspace_bytes(6), Some(()));
        assert_eq!(budget.charge_font_width_workspace_bytes(4), Some(()));
        assert_eq!(budget.charge_font_width_workspace_bytes(1), None);
        assert_eq!(budget.charge_font_width_workspace_bytes(99), None);
        assert_eq!(budget.charge_font_metrics_bytes(1), None);
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "font width workspace bytes",
                limit: 10,
                actual: 11
            })
        ));
    }

    proptest! {
        /// 入力内容と圧縮率によらず展開の長さが境界規則に従う。
        #[test]
        fn stream_limit_matches_expanded_size(bytes in prop::collection::vec(any::<u8>(), 0..20000), limit in 0u64..20000) {
            let budget = DecodeBudget::new(&ParseLimits { max_decoded_stream_bytes: limit, ..ParseLimits::default() });
            let result = decode_zlib(&compressed(&bytes), &budget);
            if bytes.len() as u64 <= limit {
                prop_assert_eq!(result, Some(bytes));
                prop_assert!(budget.check().is_ok());
            } else {
                prop_assert!(result.is_none());
                prop_assert!(matches!(budget.check(), Err(ParseError::ResourceLimit { resource: "decoded stream bytes", .. })), "limit error was lost");
            }
        }
    }
}

#[cfg(test)]
#[path = "decode_cancellation_tests.rs"]
mod cancellation_tests;

#[cfg(test)]
#[path = "decode_allocation_tests.rs"]
mod allocation_tests;
