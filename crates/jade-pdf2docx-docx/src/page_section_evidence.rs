//! 実際のWord本文XMLから、境界段落自身を含むsection所属を独立して読む。
/// 原本ページに対応するsectionの内容と段数。
#[derive(Debug)]
pub(super) struct Evidence {
    /// このsectionに所属する本文とセルの文字。
    pub(super) text: String,
    /// Wordのsection段数。未指定時は1。
    pub(super) columns: usize,
    /// 直接bodyに属する表の個数。
    pub(super) tables: usize,
    /// このsection内の画像描画の個数。
    pub(super) drawings: usize,
    /// Wordページの表示幅（twips）。
    pub(super) width: usize,
    /// Wordページの表示高さ（twips）。
    pub(super) height: usize,
}

impl Default for Evidence {
    /// Word既定の1段から検証を始める。
    fn default() -> Self {
        Self {
            text: String::new(),
            columns: 1,
            tables: 0,
            drawings: 0,
            width: 0,
            height: 0,
        }
    }
}

/// 実際のWord要素の整数属性を、prefixの表記順に依存せず読む。
fn integer(e: &quick_xml::events::BytesStart<'_>, name: &[u8], fallback: usize) -> usize {
    e.attributes()
        .map(Result::unwrap)
        .find(|a| a.key.local_name().as_ref() == name)
        .map(|a| std::str::from_utf8(&a.value).unwrap().parse().unwrap())
        .unwrap_or(fallback)
}

/// pPrのsectPrも、その段落自身を含めて前のsource sectionへ所属させる。
pub(super) fn sections(xml: &str) -> Vec<Evidence> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut pending = Evidence::default();
    let mut result = Vec::new();
    let mut boundary = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) => {
                let tag = e.local_name().as_ref().to_vec();
                if tag == b"tbl" && stack.len() == 2 {
                    pending.tables += 1;
                }
                if tag == b"drawing" {
                    pending.drawings += 1;
                }
                if tag == b"sectPr" && stack.len() == 4 && stack[2] == b"p" {
                    boundary = true;
                }
                stack.push(tag);
            }
            Event::Empty(e) if e.local_name().as_ref() == b"cols" => {
                pending.columns = integer(&e, b"num", 1);
            }
            Event::Empty(e) if e.local_name().as_ref() == b"pgSz" => {
                pending.width = integer(&e, b"w", 0);
                pending.height = integer(&e, b"h", 0);
            }
            Event::Text(e) if stack.last().is_some_and(|tag| tag == b"t") => {
                pending
                    .text
                    .push_str(std::str::from_utf8(e.as_ref()).unwrap());
                pending.text.push('\n');
            }
            Event::End(e) => {
                let tag = e.local_name();
                let paragraph_end = tag.as_ref() == b"p" && stack.len() == 3 && boundary;
                let document_end = tag.as_ref() == b"sectPr" && stack.len() == 3;
                if paragraph_end || document_end {
                    result.push(std::mem::take(&mut pending));
                    boundary = false;
                }
                stack.pop().unwrap();
            }
            Event::Eof => break,
            _ => (),
        }
    }
    result
}
