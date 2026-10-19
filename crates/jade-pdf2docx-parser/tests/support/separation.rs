//! parserとDOCXで共有する正式xrefのsampled Separation fixture。
use crate::fixture;

/// 二端点RGB tintと一成分Matte maskを持つPDFを作る。
pub fn pdf(function: &str, attributes: &str, samples: &[u8]) -> Vec<u8> {
    let mut stream = format!("<< /Length 6 {function} >>\nstream\n").into_bytes();
    stream.extend_from_slice(&[0, 128, 255, 255, 128, 0]);
    stream.extend_from_slice(b"\nendstream");
    let data = fixture::compressed(&[85, 255]);
    let mut mask=format!("<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode /Matte [.2] /Length {} >>\nstream\n",data.len()).into_bytes();
    mask.extend_from_slice(&data);
    mask.extend_from_slice(b"\nendstream");
    fixture::image_with_resources(
        &format!(
            "/ColorSpace [/Separation /Spot /DeviceRGB 12 0 R] /Filter /FlateDecode {attributes}"
        ),
        &fixture::compressed(samples),
        8,
        b"q 20 0 0 10 0 0 cm /Im Do Q",
        "<< /XObject << /Im 4 0 R >> >>",
        false,
        &[&stream, &mask],
    )
}

/// 省略値で二端点を線形補間するFunctionType 0の辞書。
pub const LINEAR_RGB: &str =
    "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [2] /BitsPerSample 8";

/// 独立profileと二端点CMYK tintを正式xrefへ登録する。
pub fn cmyk_pdf(profile: &[u8], dictionary: &str, attributes: &str) -> Vec<u8> {
    let mut function=b"<< /FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1 0 1] /Size [2] /BitsPerSample 8 /Length 8 >>\nstream\n".to_vec();
    function.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 255]);
    function.extend_from_slice(b"\nendstream");
    let data = fixture::compressed(profile);
    let mut icc = format!(
        "<< {dictionary} /Filter /FlateDecode /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    icc.extend_from_slice(&data);
    icc.extend_from_slice(b"\nendstream");
    fixture::image_with_resources(
        &format!(
            "/ColorSpace [/Separation /Black [/ICCBased 13 0 R] 12 0 R] /Filter /FlateDecode /Intent /Perceptual {attributes}"
        ),
        &fixture::compressed(&[77, 184]),
        8,
        b"q 20 0 0 10 0 0 cm /Im Do Q",
        "<< /XObject << /Im 4 0 R >> >>",
        false,
        &[&function, &icc],
    )
}
