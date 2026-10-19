//! 復号行のbyte配置を画像の1bit sample配置へ対応させる。
use super::DecodeBudget;

/// filterと画像それぞれの行幅・byte strideを保持する。
pub(super) struct Layout {
    /// 画像の行幅。
    width: u32,
    /// 画像の行数。
    height: u32,
    /// filterの行幅。
    columns: u32,
    /// filterの1行あたりのbyte数。
    filter_stride: u64,
    /// 画像の1行あたりのbyte数。
    image_stride: u64,
}

impl Layout {
    /// 1bit行末をbyte境界へ揃え、各strideをoverflowなしで求める。
    pub(super) fn new(width: u32, height: u32, columns: u32) -> Option<Self> {
        if width == 0 || height == 0 || columns == 0 {
            return None;
        }
        Some(Self {
            width,
            height,
            columns,
            filter_stride: u64::from(columns).div_ceil(8),
            image_stride: u64::from(width).div_ceil(8),
        })
    }

    /// 画像が消費するbyte列を満たす最小のfilter行数を返す。
    pub(super) fn required_rows(&self) -> Option<u32> {
        u32::try_from((self.image_stride * u64::from(self.height)).div_ceil(self.filter_stride))
            .ok()
    }

    /// packed byteを追加確保せずsample位置を対応させ、予約済み画像RGBへ保存する。
    pub(super) fn image_rgb(
        &self,
        budget: &DecodeBudget,
        decoded: &[u8],
        black_is_one: bool,
    ) -> Option<Vec<u8>> {
        let length =
            usize::try_from((u64::from(self.width) * u64::from(self.height)).checked_mul(3)?)
                .ok()?;
        let mut image = budget.image_vector(length)?;
        for row in 0..self.height {
            for x in 0..self.width {
                let byte = u64::from(row) * self.image_stride + u64::from(x / 8);
                let filter_row = byte / self.filter_stride;
                let column = (byte % self.filter_stride) * 8 + u64::from(x % 8);
                // filterの未使用の行末bitは白で埋め、BlackIs1の極性も適用する。
                let gray = if column >= u64::from(self.columns) {
                    if black_is_one { 0 } else { 255 }
                } else {
                    let index =
                        usize::try_from((filter_row * u64::from(self.columns) + column) * 3)
                            .ok()?;
                    *decoded.get(index)?
                };
                image.extend_from_slice(&[gray; 3]);
            }
        }
        Some(image)
    }
}
