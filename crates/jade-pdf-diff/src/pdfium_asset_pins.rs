// Checksum-pinned release assets. Provenance: ../pdfium-assets.json.
pub const PDFIUM_VERSION: &str = "8111";
pub const ASSETS: &[Asset] = &[
    Asset {
        os: "linux", arch: "aarch64", archive: "pdfium-linux-arm64.tgz",
        archive_bytes: 3728828, archive_sha256: "042de247138f96341534cccf24ed21c2550a7245b936bb4a60d2340120000f05",
        member: "lib/libpdfium.so", library_bytes: 8128312,
        library_sha256: "27b1f63a6ccfc7c6e93abff5a64679a982e4e0ac50235789503e25a645324f07",
    },
    Asset {
        os: "linux", arch: "x86_64", archive: "pdfium-linux-x64.tgz",
        archive_bytes: 3828739, archive_sha256: "bdc60126399f39fe768566a18d4be055a7b3681bc9e248e8a6656b4f6cae0aa2",
        member: "lib/libpdfium.so", library_bytes: 8049792,
        library_sha256: "dca41ec4a2496ea7378b144cc4bad2d9e050692aaac75b9cc22411c1835f7f6e",
    },
    Asset {
        os: "macos", arch: "aarch64", archive: "pdfium-mac-arm64.tgz",
        archive_bytes: 3541773, archive_sha256: "46c674298c60123f1195c2a03c65ee08c4783c84874ce4f5b3a054abb3ecaf30",
        member: "lib/libpdfium.dylib", library_bytes: 7388736,
        library_sha256: "baf7fdfd71c887c268224dd9fe102818ace962339875d78c2fcb0a55ba29d967",
    },
    Asset {
        os: "macos", arch: "x86_64", archive: "pdfium-mac-x64.tgz",
        archive_bytes: 3749773, archive_sha256: "e7fece4693f65a7e47f84fa38099068d573d4dcbd46b1a3c8d2c26581c308466",
        member: "lib/libpdfium.dylib", library_bytes: 7813248,
        library_sha256: "252c99942f380767453e298af395e1201fe564639f057cd0de1ac04f3f0c000c",
    },
    Asset {
        os: "windows", arch: "aarch64", archive: "pdfium-win-arm64.tgz",
        archive_bytes: 3654396, archive_sha256: "99ec8b76d5ac30602a8488da745397393ff6cddc824ef36f398146f6b4f76d4e",
        member: "bin/pdfium.dll", library_bytes: 6898688,
        library_sha256: "907a6c493e7db20d831f88cfe0ed4f0674b2e88d5e8678935dadc85ad93bc178",
    },
    Asset {
        os: "windows", arch: "x86_64", archive: "pdfium-win-x64.tgz",
        archive_bytes: 3905343, archive_sha256: "3452d11031050ec65e57f180215b57b9e6d465e3fbedafa8a77ad90885c7d23e",
        member: "bin/pdfium.dll", library_bytes: 7586816,
        library_sha256: "a318c39c137482dc4687c2973b7ec1c27fc1c20253cf2dc31928f922eab89e8f",
    },
    Asset {
        os: "windows", arch: "x86", archive: "pdfium-win-x86.tgz",
        archive_bytes: 3763621, archive_sha256: "ef298314d6e71b08e38dc90724111e37e1d29d668f739ad656e2329d491e533a",
        member: "bin/pdfium.dll", library_bytes: 7011328,
        library_sha256: "16e658836e1eb709dc280c1446fd72829d3000852df5e7a62167a861a2872d53",
    },
];
