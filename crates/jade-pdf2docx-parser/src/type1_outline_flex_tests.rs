//! 所有のFlex曲線とヒント切替だけで、承認の有無と座標の対応を検査する。
use super::*;
use crate::type1_outline::standard::Verified;
use proptest::prelude::*;

/// 整数は圧縮せず、独立した大端i32のType1表現にする。
fn numbers(bytes: &mut Vec<u8>, values: &[i32]) {
    for value in values {
        bytes.push(255);
        bytes.extend_from_slice(&value.to_be_bytes());
    }
}

/// 固定の二曲線を平行移動し、標準Subrsを介して実行する。
fn program(x: i32, y: i32) -> (Vec<u8>, Vec<Vec<u8>>) {
    let subrs = vec![
        vec![142, 139, 12, 16, 12, 17, 12, 17, 12, 33, 11],
        vec![139, 140, 12, 16, 11],
        vec![139, 141, 12, 16, 11],
        vec![11],
        vec![139, 149, 1, 11],
    ];
    let mut bytes = Vec::new();
    numbers(&mut bytes, &[0, 600]);
    bytes.push(13);
    numbers(&mut bytes, &[x, y]);
    bytes.push(21);
    bytes.extend_from_slice(&[143, 140, 142, 12, 16, 12, 17, 10]); // ヒント切替だけで位置は変えない。
    bytes.extend_from_slice(&[140, 10]);
    for pair in [
        [50, 0],
        [-30, 10],
        [20, -5],
        [10, -5],
        [10, 0],
        [20, -10],
        [20, 10],
    ] {
        numbers(&mut bytes, &pair);
        bytes.extend_from_slice(&[21, 141, 10]);
    }
    numbers(&mut bytes, &[50, x + 100, y]);
    bytes.extend_from_slice(&[139, 10]);
    numbers(&mut bytes, &[0, 30]);
    bytes.extend_from_slice(&[5, 9, 14]);
    (bytes, subrs)
}

/// 標準承認時だけ二曲線とその後の相対線分を保持する。
#[test]
fn verified_flex_preserves_curves_and_restores_currentpoint() {
    let (bytes, subrs) = program(10, 20);
    assert!(decode(&DecodeBudget::default(), &bytes, &subrs).is_none());
    assert_eq!(
        decode_verified(
            &DecodeBudget::default(),
            &bytes,
            &subrs,
            Verified::for_owned_test()
        ),
        Some(Glyph {
            advance: 600.,
            commands: vec![
                Command::Move([10., 20.]),
                Command::Curve([30., 30., 50., 25., 60., 20.]),
                Command::Curve([70., 20., 90., 10., 110., 20.]),
                Command::Line([110., 50.]),
                Command::Close
            ]
        })
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 平行移動した曲線でも、Flex後の現在点とadvanceを保つ。
    #[test]
    fn flex_translation_keeps_absolute_controls(x in -2000_i32..2000,y in -2000_i32..2000) {
        let (bytes,subrs)=program(x,y);
        let glyph=decode_verified(&DecodeBudget::default(),&bytes,&subrs,Verified::for_owned_test()).unwrap();
        prop_assert_eq!(glyph.advance,600.);
        prop_assert_eq!(&glyph.commands[1],&Command::Curve([f64::from(x+20),f64::from(y+10),f64::from(x+40),f64::from(y+5),f64::from(x+50),f64::from(y)]));
        prop_assert_eq!(&glyph.commands[3],&Command::Line([f64::from(x+100),f64::from(y+30)]));
    }
}
