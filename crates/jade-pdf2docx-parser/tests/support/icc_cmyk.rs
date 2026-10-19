//! parserとDOCXで共有する二画素のCMYK ICC画像fixture。
use crate::fixture;

/// CMYK sourceと四成分Matteのsoft maskを正式xrefへ登録する。
pub fn pdf(
    profile: &[u8],
    profile_attributes: &str,
    attributes: &str,
    samples: &[u8],
    mask_attributes: &str,
    alpha: &[u8],
) -> Vec<u8> {
    encoded_pdf(
        profile,
        profile_attributes,
        attributes,
        "FlateDecode",
        &fixture::compressed(samples),
        mask_attributes,
        alpha,
    )
}

/// codecを指定した四成分画像を、同じICCとsoft maskへ接続する。
pub fn encoded_pdf(
    profile: &[u8],
    profile_attributes: &str,
    attributes: &str,
    filter: &str,
    data: &[u8],
    mask_attributes: &str,
    alpha: &[u8],
) -> Vec<u8> {
    let compressed = fixture::compressed(profile);
    let mut icc = format!(
        "<< {profile_attributes} /Filter /FlateDecode /Length {} >>\nstream\n",
        compressed.len()
    )
    .into_bytes();
    icc.extend_from_slice(&compressed);
    icc.extend_from_slice(b"\nendstream");
    let compressed = fixture::compressed(alpha);
    let mut mask = format!(
        "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode {mask_attributes} /Length {} >>\nstream\n",
        compressed.len()
    ).into_bytes();
    mask.extend_from_slice(&compressed);
    mask.extend_from_slice(b"\nendstream");
    fixture::image_with_resources(
        &format!(
            "/ColorSpace [/ICCBased 12 0 R] /Filter /{filter} /Intent /Perceptual {attributes}"
        ),
        data,
        8,
        b"q 20 0 0 10 0 0 cm /Im Do Q",
        "<< /XObject << /Im 4 0 R >> >>",
        false,
        &[&icc, &mask],
    )
}
