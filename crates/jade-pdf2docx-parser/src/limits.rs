//! parser の入口と大きな allocation に適用する資源上限。
use crate::ParseError;

/// 呼び出し元が入力に応じて調整できる解析上限。
///
/// 共通fallible確保は累積容量を制限する。native/画像decoder内部、総RSS、実行時間は
/// この型だけでは制限されない。未信頼入力は隔離した worker で処理する。
#[derive(Debug, Clone, Copy)]
pub struct ParseLimits {
    /// PDF 入力の最大バイト数。ファイルは上限 + 1 byte まで読み込んで判定する。
    pub max_input_bytes: u64,
    /// 共通Vec/String容量と予算付きBox/Arc本体の解析ごとの累積上限。
    /// Arcは本体と二つのcounterのalignment込み容量を数え、共有cloneは再予約しない。
    /// file読込の再確保も新容量全体を数える。native/codec/allocator内部は対象外。
    pub max_total_allocation_bytes: u64,
    /// 解析可能な最大ページ数。
    pub max_pages: usize,
    /// Image XObject 辞書・JPEG/JBIG2/PNG 内部寸法・CTM 描画先の最大画素数。
    pub max_image_pixels: u64,
    /// PNG保存/配置コピー・符号化/変形・画像色変換/SMask/codec出力の累積要求容量。
    /// palette/globals・PNG読み戻しのIDAT/RGBA・fallback画素も数える。zlib展開は別予算。
    /// 再確保と解放後の再作成も数える。codec内部の作業領域とallocator内部は対象外。
    pub max_total_image_storage_bytes: u64,
    /// zlib/FlateDecode 展開・未圧縮 Contents/ObjStm/Form 本文 1 件の最大バイト数。
    pub max_decoded_stream_bytes: u64,
    /// zlib 展開・未圧縮 Contents/ObjStm/Form 保存・Form 描画走査の累積。再処理も数える。
    pub max_total_decoded_bytes: u64,
    /// Font 幅表 payload と埋め込み Encoding の対応/code space 配列の累積要求容量。
    /// 共有済み表は除き、再作成は数える。Arc/allocator の内部管理領域は含まない。
    pub max_total_font_metrics_bytes: u64,
    /// Resource 参照 cache と Font metrics cache、直接/間接Font本体/Arcの累積要求容量。再確保も数える。
    /// allocator 内部の管理領域と余分な容量は含まない。
    pub max_total_resource_workspace_bytes: u64,
    /// 構造検査のcontainer stack・stream一覧と、検査済みstreamの範囲・番号・offset・除外object配列、抽出側offset索引/旧xref一時配列、正式xrefの履歴/旧offset/一時entry/Prev集合/W・Index配列、構造入力コピー/圧縮object連結/metadata UTF-8置換、文字描画のFont名・色・q/Q保存slotと保存時の文字列コピー、metadata 索引、検証用集合・列、抽出用ページ・Resources・表示矩形・Font・Contents・XObject 配列と XObject 名の累積要求容量。
    /// 罫線のpath・塗り矩形・q/Q保存状態・ページ集約結果配列の全拡張slotも数える。本文と数値operandは借用・固定配列で読む。
    /// 公開ページ配列と、Rootなし互換ページの矩形・回転・Font Resources継承の訪問集合も数える。
    /// 訪問集合は再作成と拡張後の全slotを累積する。矩形数値は固定4要素で読む。
    /// 圧縮objectの旧xref索引・owner/member配列・結果slot・body byte配列・Rc管理領域も数える。
    /// Font定義の全候補slot・resource名と、BaseFont/Encoding object索引の番号/世代/名の全slot・名のbyte容量も数える。
    /// ページ/Form局所Font mapの全拡張slot・キー複製・BaseFont/Encoding値も数える。
    /// Formの完成Font map cache外枠も全拡張slotを数え、共有hitでは再加算しない。
    /// 全体fallbackのFont mapの全拡張slot・resource名・BaseFont/Encoding値、Font CMap object索引の番号/世代/CMapの全候補slotも数える。
    /// FontDescriptor用CMap対応mapの初回/拡張slotと再作成も数え、同じDescriptorの共有hitでは再加算しない。
    /// 埋め込みFont名のbyte容量・候補配列の初回/拡張全slot・最終結果の全候補slotも数える。
    /// Form訪問のstream順索引・ページ結果slot・改行を含む連結本文・Rc管理領域と、payload cacheの全拡張slotも数える。
    /// 共有payloadと取得済み欠落のhitは再加算せず、本文の保存・連結の再作成は累積する。
    /// Form訪問stack・q/Q保存列・ページ/FormのBT位置と描画中CTM変更点slotの初回/拡張全容量も数える。
    /// pop後は容量を再利用し、別ページ/描画での再作成を累積する。
    /// 描画Font診断の集合と診断追加時の出力配列の初回/拡張全slotも数える。
    /// Do/Tf名は本文の範囲とborrowed byte復号比較で保持し、復号名のheapを作らない。
    /// ページ/CropBox・Form BBox・文字矩形のClip頂点と各辺の交差配列も全要求容量を数える。
    /// 親Clipは借用し、頂点増加と別描画/別ページでの再作成も累積する。
    /// 未対応XObject/inline image診断の名前索引・ページ状態・重複集合・出力拡張slotも数える。
    /// 索引は既存名を借用し、Contents間のDo名も本文を借用して復号名のheapを作らない。
    /// 画像置換の出力slot・描画済みobject集合・OCR候補の整列配列も数える。
    /// 最終IRとPdfium補完後のOCR再判定の文字/可視画像ページ集合も再作成ごとに数える。
    /// 画像stream保存・配置・最終結果配列の初回/拡張全slotも数える。
    /// 配置照合は借用し、結果配列の容量不足では部分画像を返さない。
    /// 画像Doのページ状態・q/Q保存列・未描画判定集合も全要求slotを数える。
    /// Doは借用字句で走査し、token列・復号名・配置中間配列を確保しない。
    /// 文字描画operandのliteral/hex byte・CMap検索hexと分割幅・Unicode出力も数える。
    /// 本文ごとの文字結果slot、保存するFont名/色のコピーと結合時の新文字列全容量も数える。
    /// Font metrics/CIDのcode復号とページ/Form/最終文字結果の全拡張slotも数える。
    /// 空の文字集約先は予約済み本文配列を移し、slotを再確保しない。
    /// Form previewと最終preview連結の新文字列全容量も数える。
    /// 元入力の完成合成本文は共有し、再検査/更新時の再作成と共有管理領域も数える。
    /// Rootなしstream集約mapの全slotと番号順の最終配列、preview stream一覧の全拡張slotも数える。
    /// ページpreviewの重複索引slot・正規化キー・完成UTF-8の全拡張容量も数える。
    /// ページpreview候補の復号byte・Unicode出力・TJ結合の新文字列全容量も数える。
    /// previewのsource追加配列・候補/fragment配列と走査用UTF-8置換は作らない。
    /// Pdfium previewのページ連結・LF正規化出力と借用行キーの重複索引slotも数える。
    /// pure/native文字対応のページ索引/状態・文字配列・結果slot・本文/属性コピーも数える。
    /// Pdfiumのページ文字indexは不可視判定と抽出で共有し、i32配列を確保前に予約する。
    /// 通常segmentの保存slot・本文/属性コピーと文書集約の拡張容量も数える。
    /// segmentの先頭文字検索ではindex配列を作らず、共有ページindexを使う。
    /// nativeページ保存の全slotも予約し、寸法/回転の合成は既存配列を直接更新する。
    /// Pdfium API/色変換が作る一時文字列とnative内部の確保は対象外。
    /// PNGコピーは画像保存予算で制限する。他の描画resource名は対象外。
    /// 再作成・再確保と上書き前の entry、別ページの再検証も数える。
    /// allocator 内部の管理領域と余分な容量は含まない。
    pub max_total_metadata_workspace_bytes: u64,
    /// 埋め込み Font の保存・修復配列の累積要求容量。再確保と破棄した候補も数える。
    /// allocator 内部の管理領域と余分な容量は含まない。
    pub max_total_embedded_font_bytes: u64,
    /// CID 幅表を作る assignment/successor 一時配列の累積バイト数。
    pub max_total_font_width_workspace_bytes: u64,
    /// ToUnicode の code space・bfchar・bfrange 展開の累積処理数。無効値と再解析も数える。
    pub max_total_tounicode_entries: u64,
    /// ToUnicode 解析で作る key と UTF-8 value payload の累積バイト数。上書きと再解析も数える。
    /// lookup record/code space は保存容量予算で別途制限する。完成 CMap の clone は保存領域を共有する。
    pub max_total_tounicode_bytes: u64,
    /// ToUnicode lookup record・code space・stream cache 配列の累積要求容量。再確保・再解析も数える。
    /// key/value payload と allocator 内部の管理領域・余分な容量は別扱い。
    pub max_total_tounicode_storage_bytes: u64,
    /// CMap 選択/本文解析の入力サイズと不正 UTF-8 置換容量の累積。再走査も数える。
    /// 元 PDF の選択走査は入力全体を予約し、未圧縮 CMap 本文は借用する。
    pub max_total_tounicode_input_bytes: u64,
    /// dictionary/array・page tree・Form呼び出し・局所resource最短到達・q/Q stackの最大深度。
    /// Rootなしページの幾何属性/Font Resources継承も親参照数を同じ上限で制限する。
    pub max_container_nesting: usize,
    /// xref index の最大 entry 数（free・過去 revision を含む）と page tree の最大 node 数。
    pub max_xref_entries: usize,
}

impl Default for ParseLimits {
    /// 共通確保容量累積2 GiB、入力 128 MiB、1,000 ページ、画像 4,000 万画素、展開 64/累積 512 MiB、metadata 索引/検証 workspace 累積128 MiB、埋め込み Font 保存/修復 累積256 MiB、Font 幅表/Resource・Font cache/一時配列/ToUnicode payload/保存配列 累積それぞれ 64 MiB、ToUnicode 入力/置換容量 累積512 MiB・100万処理、入れ子 256、xref 100 万 entry を設定する。
    fn default() -> Self {
        Self {
            max_input_bytes: 128 * 1024 * 1024,
            max_total_allocation_bytes: 2 * 1024 * 1024 * 1024,
            max_pages: 1000,
            max_image_pixels: 40_000_000,
            max_total_image_storage_bytes: 512 * 1024 * 1024,
            max_decoded_stream_bytes: 64 * 1024 * 1024,
            max_total_decoded_bytes: 512 * 1024 * 1024,
            max_total_font_metrics_bytes: 64 * 1024 * 1024,
            max_total_resource_workspace_bytes: 64 * 1024 * 1024,
            max_total_metadata_workspace_bytes: 128 * 1024 * 1024,
            max_total_embedded_font_bytes: 256 * 1024 * 1024,
            max_total_font_width_workspace_bytes: 64 * 1024 * 1024,
            max_total_tounicode_entries: 1_000_000,
            max_total_tounicode_bytes: 64 * 1024 * 1024,
            max_total_tounicode_storage_bytes: 64 * 1024 * 1024,
            max_total_tounicode_input_bytes: 512 * 1024 * 1024,
            max_container_nesting: 256,
            max_xref_entries: 1_000_000,
        }
    }
}

/// 上限は inclusive とし、超過した資源名・上限・観測値を返す。
pub(crate) fn check_limit(
    resource: &'static str,
    actual: u64,
    limit: u64,
) -> Result<(), ParseError> {
    if actual > limit {
        Err(ParseError::ResourceLimit {
            resource,
            limit,
            actual,
        })
    } else {
        Ok(())
    }
}
