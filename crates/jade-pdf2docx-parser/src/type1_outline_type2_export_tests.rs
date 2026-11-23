//! 元FontFileと所有の端数座標をType2へ格納し、独立した読取ツールへ渡す。
use super::*;
use crate::type1_outline::type2;

/// 所有の空文字・多数の端数線分・三次曲線を、外部検査用に生成する。
pub(super) fn owned_cases() -> Vec<(String, Glyph)> {
    let mut cases = vec![
        (
            "empty-zero".to_string(),
            Glyph {
                advance: 0.,
                commands: vec![],
            },
        ),
        (
            "empty-wide".to_string(),
            Glyph {
                advance: 65535.,
                commands: vec![],
            },
        ),
        (
            "empty-fraction".to_string(),
            Glyph {
                advance: 600.25,
                commands: vec![],
            },
        ),
    ];
    let mut commands = vec![Command::Move([0.1, 0.2])];
    for index in 1..=5000 {
        commands.push(Command::Line([
            f64::from(index) / 10.,
            f64::from(index % 9) / 7.,
        ]));
    }
    commands.extend([
        Command::Close,
        Command::Move([700.1, 10.2]),
        Command::Line([710.3, 20.4]),
        Command::Close,
    ]);
    cases.push((
        "5000-fractional-lines-and-second-contour".to_string(),
        Glyph {
            advance: 65535.,
            commands,
        },
    ));
    let mut commands = vec![Command::Move([0.1, 0.2])];
    for index in 0..200 {
        let x = f64::from(index) / 7.;
        commands.push(Command::Curve([
            x + 0.1,
            1. / 3.,
            x + 0.2,
            2. / 7.,
            x + 0.3,
            3. / 11.,
        ]));
    }
    commands.push(Command::Close);
    cases.push((
        "200-fractional-curves".to_string(),
        Glyph {
            advance: 600.,
            commands,
        },
    ));
    cases
}

/// 元の座標と完成programを渡し、検査側が独立にwidthと全座標を復元できるようにする。
pub(super) fn record(source: &str, name: &str, glyph: &Glyph) -> serde_json::Value {
    let program =
        type2::encode(&DecodeBudget::default(), glyph).expect("complete bounded Type2 program");
    let commands = glyph
        .commands
        .iter()
        .map(|command| {
            let (op, points): (&str, &[f64]) = match command {
                Command::Move(p) => ("Move", p),
                Command::Line(p) => ("Line", p),
                Command::Curve(p) => ("Curve", p),
                Command::Close => ("Close", &[]),
            };
            serde_json::json!({"op":op,"points":points})
        })
        .collect::<Vec<_>>();
    serde_json::json!({"source":source,"name":name,"advance":glyph.advance,"commands":commands,"program":program})
}

/// 全351glyphは元FontFileから復号し、独立goldに一致した外形だけをexportする。
#[test]
#[ignore = "explicit reviewed original FontFile gold and unique JADE_TYPE2_EXPORT destination required"]
fn export_original_and_owned_type2_for_independent_reader() {
    let path = std::env::var("JADE_TYPE1_OUTLINE_GOLD").unwrap();
    let output = std::env::var("JADE_TYPE2_EXPORT").unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() <= 8 * 1024 * 1024);
    let gold: Gold = serde_json::from_slice(&bytes).unwrap();
    let mut records = Vec::new();
    for font in gold.fonts {
        for (name, glyph) in compare_original(&font) {
            records.push(record(&font.source_name, &name, &glyph));
        }
    }
    assert_eq!(records.len(), 351);
    for (name, glyph) in owned_cases() {
        records.push(record("owned", &name, &glyph));
    }
    let hash = hmac_sha256::Hash::hash(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    serde_json::to_writer(file,&serde_json::json!({"source_gold_sha256":hash,"nominal_width":type2::NOMINAL_WIDTH,"default_width":0,"records":records})).unwrap();
}
