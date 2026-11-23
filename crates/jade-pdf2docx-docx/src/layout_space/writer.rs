//! 所有する矩形.notdefと空のspaceだけを持つ、小さい固定構成のSFNTを組み立てる。

/// 内部で選んだASCII familyをname tableへ入れ、10表のTrueTypeを返す。
pub(super) fn encode(family: &str) -> Vec<u8> {
    assert!(
        family.len() < 63
            && family
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    );
    let mut head = vec![0; 54];
    put32(&mut head, 0, 0x10000);
    put32(&mut head, 4, 0x10000);
    put32(&mut head, 12, 0x5f0f3cf5);
    for (offset, value) in [(16, 3), (18, 1000), (40, 500), (42, 700), (46, 8), (48, 2)] {
        put16(&mut head, offset, value);
    }
    let mut hhea = vec![0; 36];
    put32(&mut hhea, 0, 0x10000);
    for (offset, value) in [
        (4, 800),
        (6, (-200i16) as u16),
        (10, 1000),
        (16, 500),
        (18, 1),
        (34, 2),
    ] {
        put16(&mut hhea, offset, value);
    }
    let mut maxp = vec![0; 32];
    put32(&mut maxp, 0, 0x10000);
    for (offset, value) in [(4, 2), (6, 4), (8, 1), (14, 2)] {
        put16(&mut maxp, offset, value);
    }
    let mut os2 = vec![0; 96];
    for (offset, value) in [
        (0, 4),
        (2, 1000),
        (4, 400),
        (6, 5),
        (62, 64),
        (64, 32),
        (66, 32),
        (68, 800),
        (70, (-200i16) as u16),
        (74, 800),
        (76, 200),
        (88, 700),
        (92, 32),
        (94, 1),
    ] {
        put16(&mut os2, offset, value);
    }
    put32(&mut os2, 42, 1); // Basic Latin
    put32(&mut os2, 78, 1); // Latin 1
    os2[58..62].copy_from_slice(b"Jade");
    let mut post = vec![0; 32];
    put32(&mut post, 0, 0x30000);
    let mut cmap = Vec::new();
    words(&mut cmap, &[0, 1, 3, 1]);
    cmap.extend_from_slice(&12u32.to_be_bytes());
    words(
        &mut cmap,
        &[
            4,
            32,
            0,
            4,
            4,
            1,
            0,
            32,
            65535,
            0,
            32,
            65535,
            (-31i16) as u16,
            1,
            0,
            0,
        ],
    );
    let mut glyf = Vec::new();
    words(&mut glyf, &[1, 0, 0, 500, 700, 3, 0]);
    glyf.extend_from_slice(&[0x31, 0x21, 0x11, 0x21]);
    words(&mut glyf, &[500, (-500i16) as u16, 700]);
    let mut hmtx = Vec::new();
    words(&mut hmtx, &[500, 0, 1000, 0]);
    let mut loca = Vec::new();
    words(&mut loca, &[0, 12, 12]);
    let tables = [
        (*b"OS/2", os2),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"name", names(family)),
        (*b"post", post),
    ];
    let mut output = vec![0; 12 + 16 * tables.len()];
    put32(&mut output, 0, 0x10000);
    for (offset, value) in [(4, 10), (6, 128), (8, 3), (10, 32)] {
        put16(&mut output, offset, value);
    }
    let mut head_offset = 0;
    for (index, (tag, data)) in tables.into_iter().enumerate() {
        let entry = 12 + 16 * index;
        let start = output.len();
        output[entry..entry + 4].copy_from_slice(&tag);
        put32(&mut output, entry + 4, checksum(&data));
        put32(&mut output, entry + 8, start as u32);
        put32(&mut output, entry + 12, data.len() as u32);
        if &tag == b"head" {
            head_offset = start;
        }
        output.extend_from_slice(&data);
        while output.len() % 4 != 0 {
            output.push(0);
        }
    }
    let adjustment = 0xb1b0afbau32.wrapping_sub(checksum(&output));
    put32(&mut output, head_offset + 8, adjustment);
    output
}

/// Windows Unicodeの日本語環境でも同じfamilyを解決できる、標準name recordを作る。
fn names(family: &str) -> Vec<u8> {
    let records = [
        (
            0,
            "Copyright Jade contributors. Owned layout font; MIT license.",
        ),
        (1, family),
        (2, "Regular"),
        (3, family),
        (4, family),
        (5, "Version 1.0"),
        (6, family),
    ];
    let mut output = Vec::new();
    words(
        &mut output,
        &[0, records.len() as u16, (6 + 12 * records.len()) as u16],
    );
    let mut strings = Vec::new();
    for (id, value) in records {
        let bytes = value
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        words(
            &mut output,
            &[3, 1, 0x409, id, bytes.len() as u16, strings.len() as u16],
        );
        strings.extend_from_slice(&bytes);
    }
    output.extend_from_slice(&strings);
    output
}

/// 表末尾のpaddingをゼロとみなし、32bitのSFNT checksumを求める。
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// 固定配置の表へbig-endianの16bit値を書く。
fn put16(out: &mut [u8], offset: usize, value: u16) {
    out[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}

/// 固定配置の表へbig-endianの32bit値を書く。
fn put32(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

/// 小さい固定列を検査済みの表へ追記する。
fn words(out: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        out.extend_from_slice(&value.to_be_bytes());
    }
}
