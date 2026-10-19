use jade_pdf2docx_ocr::VerifiedBlankPage;
use proptest::prelude::*;

/// 全白の非空RGB画像だけが白紙の証明を作れる。
#[test]
fn blank_proof_requires_exact_complete_white_rgb() {
    assert!(VerifiedBlankPage::from_rgb8(&[255; 6], 2, 1).is_some());
    for (rgb, width, height) in [
        (vec![], 0, 1),
        (vec![], 1, 0),
        (vec![255; 5], 2, 1),
        (vec![255; 7], 2, 1),
        (vec![254; 6], 2, 1),
        (vec![], usize::MAX, 2),
    ] {
        assert!(VerifiedBlankPage::from_rgb8(&rgb, width, height).is_none());
    }
}

proptest! {
    /// 1成分でも白以外なら、位置や強度にかかわらず白紙として削除しない。
    #[test]
    fn any_nonwhite_component_prevents_blank_proof(
        width in 1usize..40, height in 1usize..40, index in any::<usize>(), value in 0u8..255,
    ) {
        let mut rgb = vec![255; width * height * 3];
        let offset = index % rgb.len();
        rgb[offset] = value;
        prop_assert!(VerifiedBlankPage::from_rgb8(&rgb, width, height).is_none());
    }
}
