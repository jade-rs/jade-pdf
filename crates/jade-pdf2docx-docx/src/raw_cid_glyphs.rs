//! ToUnicode の無い Identity 符号化の CID フォントの字（私用面 U+F0000 + CID）を本来の字へ戻す。
//!
//! DynaFont（DF 平成明朝など）の拡張字形の CID は Adobe-Japan1 の範囲（〜23057）の外にあり、
//! 標準の表では引けない。項目記号・括弧の部品・異体字など、原本の字形と照らして確かめられた
//! 対応だけを戻す（lawrenceanum の kanpo-parser が官報の原本画像で確かめた表に基づく）。
//! 分からない字は無関係な漢字にせず、げた記号（〓）にする。
use jade_pdf2docx_parser::RAW_CID_BASE;

/// 対応の分からない字の代わりに置く記号。
const UNKNOWN_GLYPH: char = '〓';

/// 私用面へ移した CID の字か。
pub(crate) fn raw_cid(ch: char) -> Option<u32> {
    (ch as u32)
        .checked_sub(RAW_CID_BASE)
        .filter(|cid| *cid <= 0xFFFD)
}

/// 全角数字の文字列。
fn full_width(n: u32) -> String {
    n.to_string()
        .chars()
        .map(|d| char::from_u32(d as u32 - '0' as u32 + '０' as u32).unwrap_or(d))
        .collect()
}

/// 文字列の `index` 番目の字。
fn nth(s: &str, index: u32) -> Option<char> {
    s.chars().nth(index as usize)
}

/// 1 字の CID（Pdfium が符号位置として返した値）に対応する文字列。
fn single_glyph(code: u32) -> Option<String> {
    let text = match code {
        // 目次のページ番号の数字（〇〜九）
        0x639F..=0x63A8 => nth("〇一二三四五六七八九", code - 0x639F)?.to_string(),
        // (1)〜(10)
        0x630E..=0x6317 => format!("（{}）", full_width(code - 0x630D)),
        // ⑪〜⑯（⑩に続く）
        0x6318..=0x631D => char::from_u32(0x246A + code - 0x6318)?.to_string(),
        // ①〜⑳
        0x632D..=0x6340 => char::from_u32(0x2460 + code - 0x632D)?.to_string(),
        // (i)〜(x)
        0x6395..=0x639E => {
            const ROMAN: [&str; 10] = ["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];
            format!("（{}）", ROMAN[(code - 0x6395) as usize])
        }
        // ㈠〜㈩
        0x63B4..=0x63BD => format!("（{}）", nth("一二三四五六七八九十", code - 0x63B4)?),
        // (十一)〜(十五)
        0x63BF..=0x63C3 => format!("（十{}）", nth("一二三四五", code - 0x63BF)?),
        // (ア)〜(ワ)
        0x6541..=0x656C => format!(
            "（{}）",
            nth(
                "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワ",
                code - 0x6541
            )?
        ),
        // (あ)〜(わ)
        0x650E..=0x6538 => format!(
            "（{}）",
            nth(
                "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわ",
                code - 0x650E
            )?
        ),
        0x6570 => "（ヲ）".to_string(),
        // 縦組み数字の小数点
        0x65E0 => "・".to_string(),
        // 横組み数字の小数点・桁区切り（CID 26064・26065）
        0x65D0 => ".".to_string(),
        0x65D1 => ",".to_string(),
        // 化学名・型式のハイフン（CID 24954・24952）
        0x617A | 0x6178 => "－".to_string(),
        // 化学名のプライム
        0x65C9 => "′".to_string(),
        // ㊟
        0x629D => "（注）".to_string(),
        // (Ⅰ)・(Ⅱ)
        0x638B => "（Ⅰ）".to_string(),
        0x638C => "（Ⅱ）".to_string(),
        // 負の数の印（縦組み用の字形）
        0x624D => "△".to_string(),
        // 2 行以上にまたがる大きな括弧の部品。最初の部品だけを括弧にする
        0x6645 | 0x6646 | 0x6649 | 0x6653 => "（".to_string(),
        0x6647 | 0x6648 | 0x664B | 0x6656 => "）".to_string(),
        0x664A | 0x664C | 0x6654 | 0x6655 | 0x6657 | 0x6658 => String::new(),
        // 行をくくる波括弧・飾り罫の部品（字ではない）
        0x62FC..=0x6303 | 0x65B3 | 0x65B4 | 0x668A | 0x668D | 0x668F | 0x6692 | 0x6699..=0x669F => {
            String::new()
        }
        // 人名・地名の異体字など
        0x41BE => "婷".to_string(),
        0x41FC => "暻".to_string(),
        0x4239 => "濰".to_string(),
        0x4306 => "鄔".to_string(),
        0x4322 => "雯".to_string(),
        0x4473 => "宁".to_string(),
        0x44C6 => "廻".to_string(),
        0x4B17 => "吴".to_string(),
        0x4F2E => "灸".to_string(),
        0x4F2F => "厩".to_string(),
        0x4F34 => "鍵".to_string(),
        0x4F37 => "梗".to_string(),
        0x4F38 => "鮫".to_string(),
        0x4F3A => "杖".to_string(),
        0x4F3D => "竈".to_string(),
        0x4F41 => "斧".to_string(),
        0x4F42 => "釜".to_string(),
        0x4F45 => "籾".to_string(),
        0x4F46 => "爺".to_string(),
        0x52C7 => "刘".to_string(),
        0x5305 => "启".to_string(),
        0x5347 => "團".to_string(),
        0x55AD => "煒".to_string(),
        0x5A20 => "芦".to_string(),
        0x5A31 => "葛".to_string(),
        0x5A37 => "祇".to_string(),
        0x5A43 => "祁".to_string(),
        0x5A51 => "榊".to_string(),
        0x5A52 => "薩".to_string(),
        0x5A53 => "鯖".to_string(),
        0x5A69 => "箭".to_string(),
        0x5A70 => "蛸".to_string(),
        0x5A72 => "樽".to_string(),
        0x5A78 => "鎚".to_string(),
        0x5A79 => "辻".to_string(),
        0x5A7B => "鄭".to_string(),
        0x5A8D => "箸".to_string(),
        0x5A91 => "樋".to_string(),
        0x5AA1 => "蓬".to_string(),
        0x5B12 | 0x5B3D => "德".to_string(),
        _ => return None,
    };
    Some(text)
}

/// 私用面へ移した CID の字を本来の字へ戻す。罫線用フォントの字（線を字で組んだもの）は除き、
/// 対応の分からない字は〓にする。ほかの字はそのまま残す。
pub(crate) fn restore(text: &str, rule_font: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let Some(code) = raw_cid(chars[index]) else {
            out.push(chars[index]);
            index += 1;
            continue;
        };
        if rule_font {
            index += 1;
            continue;
        }
        // (11)〜(99): 「(十の位」+「一の位)」（一の位は 1〜9, 0 の順）
        if let Some(ones) = chars.get(index + 1).copied().and_then(raw_cid)
            && (0x661D..=0x6625).contains(&code)
            && (0x6627..=0x6630).contains(&ones)
        {
            let value = (code - 0x661C) * 10 + (ones - 0x6626) % 10;
            out.push_str(&format!("（{}）", full_width(value)));
            index += 2;
            continue;
        }
        match single_glyph(code) {
            Some(value) => out.push_str(&value),
            None => out.push(UNKNOWN_GLYPH),
        }
        index += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CID を私用面の字にする。
    fn raw(codes: &[u32]) -> String {
        codes
            .iter()
            .map(|code| char::from_u32(RAW_CID_BASE + code).unwrap())
            .collect()
    }

    /// 項目記号・目次の数字・負の数の印を戻し、括弧の部品は落とす。
    #[test]
    fn restores_known_dynafont_glyphs() {
        assert_eq!(restore(&raw(&[0x630E]), false), "（１）");
        assert_eq!(restore(&raw(&[0x632D, 0x6340]), false), "①⑳");
        assert_eq!(restore(&raw(&[0x63A0, 0x63A2]), false), "一三");
        assert_eq!(restore(&format!("{}100", raw(&[0x624D])), false), "△100");
        assert_eq!(restore(&raw(&[0x6300, 0x668D, 0x668A, 0x6301]), false), "");
    }

    /// 2 字で組んだ (11)〜(99) を 1 つの項目記号に戻す。
    #[test]
    fn restores_two_glyph_parenthesized_numbers() {
        assert_eq!(restore(&raw(&[0x661D, 0x6627]), false), "（１１）");
        assert_eq!(restore(&raw(&[0x661E, 0x6630]), false), "（２０）");
    }

    /// 罫線用フォントの字は落とし、分からない字は〓にする。ほかの字は残す。
    #[test]
    fn drops_rule_glyphs_and_marks_unknown_ones() {
        assert_eq!(
            restore(&format!("a{}b", raw(&[0x5A23, 0x5A23])), true),
            "ab"
        );
        assert_eq!(restore(&format!("第{}条", raw(&[0x7000])), false), "第〓条");
    }
}
