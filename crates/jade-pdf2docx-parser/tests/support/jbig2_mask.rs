//! JBIG2のPageInformationとMMR generic regionで独立な白黒画素を符号化する。
#[path = "ccitt_mask.rs"]
mod ccitt;

/// trueを黒として、同寸法のpageとlossless generic regionを作る。
pub fn encoded_mask(rows: &[Vec<bool>]) -> Vec<u8> {
    let width = u32::try_from(rows[0].len()).unwrap();
    let height = u32::try_from(rows.len()).unwrap();
    let mut page = Vec::new();
    page.extend_from_slice(&width.to_be_bytes());
    page.extend_from_slice(&height.to_be_bytes());
    page.extend_from_slice(&[0; 11]); // resolution 8 bytes、flags 1 byte、striping 2 bytes。
    let mut region = Vec::new();
    region.extend_from_slice(&width.to_be_bytes());
    region.extend_from_slice(&height.to_be_bytes());
    region.extend_from_slice(&[0; 9]); // x/y座標とOR combination operator。
    region.push(1); // MMRで符号化し、arithmetic templateは使わない。
    region.extend_from_slice(&ccitt::encoded_mask(rows));
    let mut stream = Vec::new();
    segment(&mut stream, 1, 48, &page);
    segment(&mut stream, 2, 39, &region);
    stream
}

/// 参照なし・page association 1・既知Lengthのsegmentを追記する。
fn segment(stream: &mut Vec<u8>, number: u32, kind: u8, data: &[u8]) {
    stream.extend_from_slice(&number.to_be_bytes());
    stream.extend_from_slice(&[kind, 0, 1]);
    stream.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    stream.extend_from_slice(data);
}
