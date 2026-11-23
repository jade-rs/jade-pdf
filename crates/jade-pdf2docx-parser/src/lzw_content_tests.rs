//! Rust実装より先に独立復号で確認したLZW列と、資源境界を検査する。
use super::*;
use crate::{ParseError, ParseLimits, decode::decode_lzw};
use proptest::prelude::*;

/// 所有データの固定hex表記を、追加依存なしでテスト内のbyte列へ戻す。
fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// 小さな辞書参照・clear・KwKwKの独立した期待値を比較する。
#[test]
fn independently_verified_dictionary_vectors() {
    for (name, encoded, expected) in [
        ("empty", "804040", ""),
        ("literal", "80106020", "41"),
        ("dictionary-reference", "801048502808", "41424142"),
        ("KwKwK", "8010605010", "414141"),
        ("compound-reference", "80104850282404", "41424142414241"),
        ("clear-reset", "801048500219120501", "414243444344"),
        ("binary-literals", "80001ff00003fe02", "00ff00ff"),
    ] {
        let budget = DecodeBudget::default();
        assert_eq!(
            decode_lzw(&hex(encoded), &budget, 1),
            Some(hex(expected)),
            "{name}"
        );
        assert!(budget.check().is_ok());
    }
}

/// 9・10・11・12bitへ変化する、実装前に独立復号済みの固定列を照合する。
#[test]
fn independently_verified_width_growth_vectors() {
    for (name, encoded, length) in [
        (
            "width-growth-600",
            concat!(
                "800000202018100a0603820120a058301a0e078402212098502a160b860321a0d8703a1e0f8804222118904a26138a05",
                "22a158b05a2e178c06232198d06a361b8e0723a1d8f07a3e1f9008242219108a4623920924a259309a4e27940a252299",
                "50aa562b960b25a2d970ba5e2f980c26231990ca66339a0d26a359b0da6e379c0e272399d0ea763b9e0f27a3d9f0fa7e",
                "3fa01028241a110a8643a21128a45a311a8e47a41229249a512a964ba61329a4da713a9e4fa8142a251a914aa653aa15",
                "2aa55ab15aae57ac162b259ad16ab65bae172ba5daf17abe5fb0182c261b118ac663b2192ca65b319ace67b41a2d269b",
                "51aad66bb61b2da6db71bade6fb81c2e271b91cae673ba1d2ea75bb1daee77bc1e2f279bd1eaf67bbe1f2fa000040401",
                "8080280c0381004814058180681c0782008824098280a82c0b8300c8340d8380e83c0f84010844118481284c13850148",
                "54158581685c1786018864198681a86c1b8701c8741d8781e87c1f88020884218882288c2389024894258982689c278a",
                "0288a4298a82a8ac2b8b02c8b42d8b82e8bc2f8c0308c4318c8328cc338d0348d4358d8368dc378e0388e4398e83a8ec",
                "3b8f03c8f43d8f83e8fc3f90040904419084290c4391044914459184691c4792048924499284a92c4b9304c9344d9384",
                "e93c4f94050944519485294c5395054954559585695c5796058964599685a96c5b9705c9745d9785e97c5f9806098461",
                "9886298c6399064994659986699c679a0689a4699a86a9ac6b9b06c9b46d9b86e9bc6f9c0709c4719c8729cc739d0749",
                "d4759d8769dc779e0789e4799e87a9ec7b9f07c9f400002010060200a0300e04012050160601a0701e08022090260a02",
                "a0b02e0c0320d0360e03a0f03e10042110461204a1304e14052150561605a1705e18062190661a06a1b06e1c0721d076",
                "1e07a1f07e20082210862208a2308e24092250962609a2709e280a2290a62a0aa2b0ae2c0b22d0b62e0ba2f0be300c28",
                "08",
            ),
            600usize,
        ),
        (
            "width-growth-1800",
            concat!(
                "800000202018100a0603820120a058301a0e078402212098502a160b860321a0d8703a1e0f8804222118904a26138a05",
                "22a158b05a2e178c06232198d06a361b8e0723a1d8f07a3e1f9008242219108a4623920924a259309a4e27940a252299",
                "50aa562b960b25a2d970ba5e2f980c26231990ca66339a0d26a359b0da6e379c0e272399d0ea763b9e0f27a3d9f0fa7e",
                "3fa01028241a110a8643a21128a45a311a8e47a41229249a512a964ba61329a4da713a9e4fa8142a251a914aa653aa15",
                "2aa55ab15aae57ac162b259ad16ab65bae172ba5daf17abe5fb0182c261b118ac663b2192ca65b319ace67b41a2d269b",
                "51aad66bb61b2da6db71bade6fb81c2e271b91cae673ba1d2ea75bb1daee77bc1e2f279bd1eaf67bbe1f2fa000040401",
                "8080280c0381004814058180681c0782008824098280a82c0b8300c8340d8380e83c0f84010844118481284c13850148",
                "54158581685c1786018864198681a86c1b8701c8741d8781e87c1f88020884218882288c2389024894258982689c278a",
                "0288a4298a82a8ac2b8b02c8b42d8b82e8bc2f8c0308c4318c8328cc338d0348d4358d8368dc378e0388e4398e83a8ec",
                "3b8f03c8f43d8f83e8fc3f90040904419084290c4391044914459184691c4792048924499284a92c4b9304c9344d9384",
                "e93c4f94050944519485294c5395054954559585695c5796058964599685a96c5b9705c9745d9785e97c5f9806098461",
                "9886298c6399064994659986699c679a0689a4699a86a9ac6b9b06c9b46d9b86e9bc6f9c0709c4719c8729cc739d0749",
                "d4759d8769dc779e0789e4799e87a9ec7b9f07c9f400002010060200a0300e04012050160601a0701e08022090260a02",
                "a0b02e0c0320d0360e03a0f03e10042110461204a1304e14052150561605a1705e18062190661a06a1b06e1c0721d076",
                "1e07a1f07e20082210862208a2308e24092250962609a2709e280a2290a62a0aa2b0ae2c0b22d0b62e0ba2f0be300c23",
                "10c6320ca330ce340d2350d6360da370de380e2390e63a0ea3b0ee3c0f23d0f63e0fa3f0fe40102411064210a4310e44",
                "112451164611a4711e48122491264a12a4b12e4c1324d1364e13a4f13e50142511465214a5314e54152551565615a571",
                "5e58162591665a16a5b16e5c1725d1765e17a5f17e60182611866218a6318e64192651966619a6719e681a2691a66a1a",
                "a6b1ae6c1b26d1b66e1ba6f1be701c2711c6721ca731ce741d2751d6761da771de781e2791e67a1ea7b1ee7c1f27d000",
                "00804018080280c0381004814058180340700f020044090130280540b0170300640d01b0380740f01f04008411023048",
                "094130270500a41502b0580b41702f0600c4190330680d41b0370700e41d03b0780f41f03f0801042104308811423047",
                "0901242504b0981342704f0a0144290530a81542b0570b01642d05b0b81742f05f0c0184310630c8194330670d01a435",
                "06b0d81b43706f0e01c4390730e81d43b0770f01e43d07b0f81f43f07f10020441083108214430871102244508b11823",
                "44708f120244490931282544b0971302644d09b1382744f09f140284510a3148294530a71502a4550ab1582b4570af16",
                "02c4590b31682d45b0b71702e45d0bb1782f45f0bf180304610c3188314630c7190324650cb198334670cf1a0344690d",
                "31a83546b0d71b03646d0db1b83746f0df1c0384710e31c8394730e71d03a4750eb1d83b4770ef1e03c4790f31e83d47",
                "b0f71f03e47d0000020080180400a018038080120280580c01a038078100220480981402a0580b8180320680d81c03a0",
                "780f8200420881182404a098138280520a81582c05a0b8178300620c81983406a0d81b8380720e81d83c07a0f81f8400",
                "821082184408a118238480921282584c09a138278500a2148298540aa1582b8580b21682d85c0ba1782f8600c2188318",
                "640ca198338680d21a83586c0da1b8378700e21c8398740ea1d83b8780f21e83d87c0fa1f83f8801022084188410a218",
                "438881122284588c11a238478901222484989412a2584b8981322684d89c13a2784f8a0142288518a414a298538a8152",
                "2a8558ac15a2b8578b01622c8598b416a2d85b8b81722e85d8bc17a2f85f8c0182308618c418a318638c8192328658cc",
                "19a338678d01a2348698d41aa3586b8d81b23686d8dc1ba3786f8e01c2388718e41ca398738e81d23a8758ec1da3b877",
                "8f01e23c8798f41ea3d87b8f81f23e80000100400c0200500c01c0400901402c0600d01c03c0801102404c0a01502c05",
                "c0c01903406c0e01d03c07c1002104408c1202504c09c140290540ac1602d05c0bc180310640cc1a03506c0dc1c03907",
                "40ec1e03d07c0fc2004108410c2204508c11c2404909412c2604d09c13c280510a414c2a0550ac15c2c0590b416c2e05",
                "d0bc17c300610c418c320650cc19c340690d41ac3606d0dc1bc380710e41cc3a0750ec1dc3c0790f41ec3e07d0fc1fc4",
                "008110420c4208510c21c4408911422c4608d11c23c4809112424c4a09512c25c4c09913426c4e09d13c27c500a11442",
                "8c520a514c29c540a91542ac560ad15c2bc580b11642cc5a0b516c2dc5c0b91742ec5e0bd17c2fc600c118430c620c51",
                "8c31c640c919432c660cd19c33c680d11a434c6a0d51ac35c6c0d91b436c6e0dd1bc37c700e11c438c720e51cc39c740",
                "e91d43ac760ed1dc3bc780f11e43cc7a0f51ec3dc7c0f91f4000008020060100280600e0200480a0160300680e01e040",
                "088120260500a81602e0600c81a0360700e81e03e080108220460901282604e0a01482a0560b01682e05e0c018832066",
                "0d01a83606e0e01c83a0760f01e83e07e100208420861102284608e1202484a0961302684e09e140288520a61502a856",
                "0ae1602c85a0b61702e85e0be180308620c6190328660ce1a03486a0d61b03686e0de1c0388720e61d03a8760ee1e03c",
                "87a0f61f03e87e0fe200408821062104288610e2204488a1162304688e11e240488921262504a89612e2604c89a13627",
                "04e89e13e280508a2146290528a614e2a0548aa1562b0568ae15e2c0588b21662d05a8b616e2e05c8ba1762f05e8be17",
                "e300608c2186310628c618e320648ca196330668ce19e340688d21a63506a8d61ae3606c8da1b63706e8de1be380708e",
                "21c6390728e61ce3a0748ea1d63b0768ee1de3c0788f21e63d07a8f61ee3e07c8fa00000401003008014030070100240",
                "500b0180340700f020044090130280540b0170300640d01b0380740f01f04004204404604804a04c04e0500520542020",
            ),
            1800usize,
        ),
        (
            "width-growth-3500",
            concat!(
                "800000202018100a0603820120a058301a0e078402212098502a160b860321a0d8703a1e0f8804222118904a26138a05",
                "22a158b05a2e178c06232198d06a361b8e0723a1d8f07a3e1f9008242219108a4623920924a259309a4e27940a252299",
                "50aa562b960b25a2d970ba5e2f980c26231990ca66339a0d26a359b0da6e379c0e272399d0ea763b9e0f27a3d9f0fa7e",
                "3fa01028241a110a8643a21128a45a311a8e47a41229249a512a964ba61329a4da713a9e4fa8142a251a914aa653aa15",
                "2aa55ab15aae57ac162b259ad16ab65bae172ba5daf17abe5fb0182c261b118ac663b2192ca65b319ace67b41a2d269b",
                "51aad66bb61b2da6db71bade6fb81c2e271b91cae673ba1d2ea75bb1daee77bc1e2f279bd1eaf67bbe1f2fa000040401",
                "8080280c0381004814058180681c0782008824098280a82c0b8300c8340d8380e83c0f84010844118481284c13850148",
                "54158581685c1786018864198681a86c1b8701c8741d8781e87c1f88020884218882288c2389024894258982689c278a",
                "0288a4298a82a8ac2b8b02c8b42d8b82e8bc2f8c0308c4318c8328cc338d0348d4358d8368dc378e0388e4398e83a8ec",
                "3b8f03c8f43d8f83e8fc3f90040904419084290c4391044914459184691c4792048924499284a92c4b9304c9344d9384",
                "e93c4f94050944519485294c5395054954559585695c5796058964599685a96c5b9705c9745d9785e97c5f9806098461",
                "9886298c6399064994659986699c679a0689a4699a86a9ac6b9b06c9b46d9b86e9bc6f9c0709c4719c8729cc739d0749",
                "d4759d8769dc779e0789e4799e87a9ec7b9f07c9f400002010060200a0300e04012050160601a0701e08022090260a02",
                "a0b02e0c0320d0360e03a0f03e10042110461204a1304e14052150561605a1705e18062190661a06a1b06e1c0721d076",
                "1e07a1f07e20082210862208a2308e24092250962609a2709e280a2290a62a0aa2b0ae2c0b22d0b62e0ba2f0be300c23",
                "10c6320ca330ce340d2350d6360da370de380e2390e63a0ea3b0ee3c0f23d0f63e0fa3f0fe40102411064210a4310e44",
                "112451164611a4711e48122491264a12a4b12e4c1324d1364e13a4f13e50142511465214a5314e54152551565615a571",
                "5e58162591665a16a5b16e5c1725d1765e17a5f17e60182611866218a6318e64192651966619a6719e681a2691a66a1a",
                "a6b1ae6c1b26d1b66e1ba6f1be701c2711c6721ca731ce741d2751d6761da771de781e2791e67a1ea7b1ee7c1f27d000",
                "00804018080280c0381004814058180340700f020044090130280540b0170300640d01b0380740f01f04008411023048",
                "094130270500a41502b0580b41702f0600c4190330680d41b0370700e41d03b0780f41f03f0801042104308811423047",
                "0901242504b0981342704f0a0144290530a81542b0570b01642d05b0b81742f05f0c0184310630c8194330670d01a435",
                "06b0d81b43706f0e01c4390730e81d43b0770f01e43d07b0f81f43f07f10020441083108214430871102244508b11823",
                "44708f120244490931282544b0971302644d09b1382744f09f140284510a3148294530a71502a4550ab1582b4570af16",
                "02c4590b31682d45b0b71702e45d0bb1782f45f0bf180304610c3188314630c7190324650cb198334670cf1a0344690d",
                "31a83546b0d71b03646d0db1b83746f0df1c0384710e31c8394730e71d03a4750eb1d83b4770ef1e03c4790f31e83d47",
                "b0f71f03e47d0000020080180400a018038080120280580c01a038078100220480981402a0580b8180320680d81c03a0",
                "780f8200420881182404a098138280520a81582c05a0b8178300620c81983406a0d81b8380720e81d83c07a0f81f8400",
                "821082184408a118238480921282584c09a138278500a2148298540aa1582b8580b21682d85c0ba1782f8600c2188318",
                "640ca198338680d21a83586c0da1b8378700e21c8398740ea1d83b8780f21e83d87c0fa1f83f8801022084188410a218",
                "438881122284588c11a238478901222484989412a2584b8981322684d89c13a2784f8a0142288518a414a298538a8152",
                "2a8558ac15a2b8578b01622c8598b416a2d85b8b81722e85d8bc17a2f85f8c0182308618c418a318638c8192328658cc",
                "19a338678d01a2348698d41aa3586b8d81b23686d8dc1ba3786f8e01c2388718e41ca398738e81d23a8758ec1da3b877",
                "8f01e23c8798f41ea3d87b8f81f23e80000100400c0200500c01c0400901402c0600d01c03c0801102404c0a01502c05",
                "c0c01903406c0e01d03c07c1002104408c1202504c09c140290540ac1602d05c0bc180310640cc1a03506c0dc1c03907",
                "40ec1e03d07c0fc2004108410c2204508c11c2404909412c2604d09c13c280510a414c2a0550ac15c2c0590b416c2e05",
                "d0bc17c300610c418c320650cc19c340690d41ac3606d0dc1bc380710e41cc3a0750ec1dc3c0790f41ec3e07d0fc1fc4",
                "008110420c4208510c21c4408911422c4608d11c23c4809112424c4a09512c25c4c09913426c4e09d13c27c500a11442",
                "8c520a514c29c540a91542ac560ad15c2bc580b11642cc5a0b516c2dc5c0b91742ec5e0bd17c2fc600c118430c620c51",
                "8c31c640c919432c660cd19c33c680d11a434c6a0d51ac35c6c0d91b436c6e0dd1bc37c700e11c438c720e51cc39c740",
                "e91d43ac760ed1dc3bc780f11e43cc7a0f51ec3dc7c0f91f4000008020060100280600e0200480a0160300680e01e040",
                "088120260500a81602e0600c81a0360700e81e03e080108220460901282604e0a01482a0560b01682e05e0c018832066",
                "0d01a83606e0e01c83a0760f01e83e07e100208420861102284608e1202484a0961302684e09e140288520a61502a856",
                "0ae1602c85a0b61702e85e0be180308620c6190328660ce1a03486a0d61b03686e0de1c0388720e61d03a8760ee1e03c",
                "87a0f61f03e87e0fe200408821062104288610e2204488a1162304688e11e240488921262504a89612e2604c89a13627",
                "04e89e13e280508a2146290528a614e2a0548aa1562b0568ae15e2c0588b21662d05a8b616e2e05c8ba1762f05e8be17",
                "e300608c2186310628c618e320648ca196330668ce19e340688d21a63506a8d61ae3606c8da1b63706e8de1be380708e",
                "21c6390728e61ce3a0748ea1d63b0768ee1de3c0788f21e63d07a8f61ee3e07c8fa00000401003008014030070100240",
                "500b0180340700f020044090130280540b0170300640d01b0380740f01f04004204404604804a04c04e0500520540560",
                "5805a05c05e06006206406606806a06c06e07007207407607807a07c07e08008208408608808a08c08e0900920940960",
                "9809a09c09e0a00a20a40a60a80aa0ac0ae0b00b20b40b60b80ba0bc0be0c00c20c40c60c80ca0cc0ce0d00d20d40d60",
                "d80da0dc0de0e00e20e40e60e80ea0ec0ee0f00f20f40f60f80fa0fc0fe10010210410610810a10c10e1101121141161",
                "1811a11c11e12012212412612812a12c12e13013213413613813a13c13e14014214414614814a14c14e1501521541561",
                "5815a15c15e16016216416616816a16c16e17017217417617817a17c17e18018218418618818a18c18e1901921941961",
                "9819a19c19e1a01a21a41a61a81aa1ac1ae1b01b21b41b61b81ba1bc1be1c01c21c41c61c81ca1cc1ce1d01d21d41d61",
                "d81da1dc1de1e01e21e41e61e81ea1ec1ee1f01f21f400000200400600800a00c00e01001201401601801a01c01e0200",
                "2202402602802a02c02e03003203403603803a03c03e04004204404604804a04c04e05005205405605805a05c05e0600",
                "6206406606806a06c06e07007207407607807a07c07e08008208408608808a08c08e09009209409609809a09c09e0a00",
                "a20a40a60a80aa0ac0ae0b00b20b40b60b80ba0bc0be0c00c20c40c60c80ca0cc0ce0d00d20d40d60d80da0dc0de0e00",
                "e20e40e60e80ea0ec0ee0f00f20f40f60f80fa0fc0fe10010210410610810a10c10e11011211411611811a11c11e1201",
                "2212412612812a12c12e13013213413613813a13c13e14014214414614814a14c14e15015215415615815a15c15e1601",
                "6216416616816a16c16e17017217417617817a17c17e18018218418618818a18c18e19019219419619819a19c19e1a01",
                "a21a41a61a81aa1ac1ae1b01b21b41b61b81ba1bc1be1c01c21c41c61c81ca1cc1ce1d01d21d41d61d81da1dc1de1e01",
                "e21e41e61e81ea1ec1ee1f01f21f400000200400600800a00c00e01001201401601801a01c01e02002202402602802a0",
                "2c02e03003203403603803a03c03e04004204404604804a04c04e05005205405605805a05c05e06006206406606806a0",
                "6c06e07007207407607807a07c07e08008208408608808a08c08e09009209409609809a09c09e0a00a20a40a60a80aa0",
                "ac0ae0b00b20b40b60b80ba0bc0be0c00c20c40c60c80ca0cc0ce0d00d20d40d60d80da0dc0de0e00e20e40e60e80ea0",
                "ec0ee0f00f20f40f60f80fa0fc0fe10010210410610810a10c10e11011211411611811a11c11e12012212412612812a1",
                "2c12e13013213413613813a13c13e14014214414614814a14c14e15015215415615815a15c15e16016216416616816a1",
                "6c16e17017217417617817a17c17e18018218418618818a18c18e19019219419619819a19c19e1a01a21a41a61a81aa1",
                "ac1ae1b01b21b41b61b81ba1bc1be1c01c21c41c61c81ca1cc1ce1d01d21d41d61d81da1dc1de1e01e21e41e61e81ea1",
                "ec1ee1f01f21f400000200400600800a00c00e01001201401601801a01c01e02002202402602802a02c02e0300320340",
                "3603803a03c03e04004204404604804a04c04e05005205405605805a05c05e06006206406606806a06c06e0700720740",
                "7607807a07c07e08008208408608808a08c08e09009209409609809a09c09e0a00a20a40a60a80aa0ac0ae0b00b20b40",
                "b60b80ba0bc0be0c00c20c40c60c80ca0cc0ce0d00d20d40d60d80da0dc0de0e00e20e40e60e80ea0ec0ee0f00f20f40",
                "f60f80fa0fc0fe10010210410610810a10c10e11011211411611811a11c11e12012212412612812a12c12e1301321341",
                "3613813a13c13e14014214414614814a14c14e15015215415615815a15c15e16016216416616816a16c16e1701721741",
                "7617817a17c17e18018218418618818a18c18e19019219419619819a19c19e1a01a21a41a61a81aa1ac1ae1b01b21b41",
                "b61b81ba1bc1be1c01c21c41c61c81ca1cc1ce1d01d21d41d61d81da1dc1de1e01e21e41e61e81ea1ec1ee1f01f21f40",
                "0000200400600800a00c00e01001201401601801a01c01e02002202402602802a02c02e03003203403603803a03c03e0",
                "4004204404604804a04c04e05005205405605805a05c05e06006206406606806a06c06e07007207407607807a07c07e0",
                "8008208408608808a08c08e09009209409609809a09c09e0a00a20a40a60a80aa0ac0ae0b00b20b40b60b80ba0bc0be0",
                "c00c20c40c60c80ca0cc0ce0d00d20d40d60d80da0dc0de0e00e20e40e60e80ea0ec0ee0f00f20f40f60f80fa0fc0fe1",
                "0010210410610810a10c10e11011211411611811a11c11e12012212412612812a12c12e13013213413613813a13c13e1",
                "4014214414614814a14c14e15015215415615815a15c15e16016216416616816a16c16e17017217417617817a17c17e1",
                "8018218418618818a18c18e19019219419619819a19c19e1a01a21a41a61a81aa1ac1ae1b01b21b41b61b81ba1bc1be1",
                "c01c21c41c61c81ca1cc1ce1d01d21d41d61d81da1dc1de1e01e21e41e61e81ea1ec1ee1f01f21f40000020040060080",
                "0a00c00e01001201401601801a01c01e02002202402602802a02c02e03003203403603803a03c03e0400420440460480",
                "4a04c04e05005205405605805a05c05e06006206406606806a06c06e07007207407607807a07c07e0800820840860880",
                "8a08c08e09009209409609809a09c09e0a00a20a40a60a80aa0ac0ae0b00b20b40b60b80ba0bc0be0c00c20c40c60c80",
                "ca0cc0ce0d00d20d40d60d80da0dc0de0e00e20e40e60e80ea0ec0ee0f00f20f40f60f80fa0fc0fe1001021041061081",
                "0a10c10e11011211411611811a11c11e12012212412612812a12c12e13013213413613813a13c13e1401421441461481",
                "4a14c14e15015215415615815a15c15e16016216416616816a16c16e17017217417617817a17c17e1801821841861881",
                "8a18c18e19019219419619819a19c19e1a01a21a41a61a81aa1ac1ae1b01b21b41b61b81ba1bc1be1c01c21c41c61c81",
                "ca1cc1ce1d01d21d41d61d81da1dc1de1e01e21e41e61e81ea1ec1ee1f01f21f400000200400600800a00c00e0100120",
                "1401601801a01c01e02002202402602802a02c02e03003203403603803a03c03e04004204404604804a04c04e0500520",
                "5405605805a05c05e06006206406606806a06c06e07007207407607807a07c07e08008208408608808a08c08e0900920",
                "9409609809a09c09e0a00a20a40a60a80aa0ac0ae0b00b20b40b60b80ba0bc0be0c00c20c40c60c80ca0cc0ce0d00d20",
                "d40d60d80da0dc0de0e00e20e40e60e80ea0ec0ee0f00f20f40f60f80fa0fc0fe10010210410610810a10c10e1101121",
                "1411611811a11c11e12012212412612812a12c12e13013213413613813a13c13e14014214414614814a14c14e1501521",
                "5415615815a15c15e16016216416616816a16c16e17017217417617817a17c17e18018218418618818a18c18e1901921",
                "9419619819a19c19e1a01a21a41a61a81aa1ac1ae1b01b21b41b61b81ba1bc1be1c01c21c41c61c81ca1cc1ce1d01d21",
                "d41d61d82020",
            ),
            3500usize,
        ),
    ] {
        let budget = DecodeBudget::default();
        let expected = (0..length)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        assert_eq!(
            decode_lzw(&hex(encoded), &budget, 1),
            Some(expected),
            "{name}"
        );
        assert!(budget.check().is_ok());
    }
}

/// 幅変更前のcode列を、独立した9bitのMSB順で所有データへ符号化する。
fn nine_bit(codes: &[u16]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut pending = 0u32;
    let mut bits = 0u8;
    for code in codes {
        assert!(*code < 512);
        pending = (pending << 9) | u32::from(*code);
        bits += 9;
        while bits >= 8 {
            bits -= 8;
            output.push((pending >> bits) as u8);
        }
        pending &= (1u32 << bits) - 1;
    }
    if bits > 0 {
        output.push((pending << (8 - bits)) as u8);
    }
    output
}

/// 空の展開でも二つの固定作業配列は確保前に共通容量へ予約する。
#[test]
fn dictionary_workspace_has_an_inclusive_allocation_boundary() {
    let input = nine_bit(&[256, 257]);
    let requested = (CAPACITY * (std::mem::size_of::<Entry>() + std::mem::size_of::<u8>())) as u64;
    let exact = DecodeBudget::new(&ParseLimits {
        max_total_allocation_bytes: requested,
        ..Default::default()
    });
    assert_eq!(decode_lzw(&input, &exact, 1), Some(Vec::new()));
    assert!(exact.check().is_ok());
    let short = DecodeBudget::new(&ParseLimits {
        max_total_allocation_bytes: requested - 1,
        ..Default::default()
    });
    assert!(decode_lzw(&input, &short, 1).is_none());
    assert!(
        matches!(short.check(), Err(ParseError::ResourceLimit { resource: "total allocation bytes", limit, actual }) if limit == requested - 1 && actual == requested)
    );
    assert!(decode_lzw(&input, &short, 1).is_none());
}

/// 未定義codeと途中で切れたcodeは、部分出力を成功として公開しない。
#[test]
fn malformed_codes_and_truncated_eod_are_not_partial_success() {
    let complete = nine_bit(&[256, 65, 257]);
    for data in [
        nine_bit(&[256, 300, 257]),
        complete[..complete.len() - 1].to_vec(),
        vec![0x80],
    ] {
        let budget = DecodeBudget::default();
        assert!(decode_lzw(&data, &budget, 1).is_none());
        assert!(
            budget.check().is_ok(),
            "codec corruption is distinct from a resource excess"
        );
    }
}

/// テストで指定したcodeとbit幅だけを、辞書処理なしにMSB-firstで詰める。
fn pack_widths(codes: &[(u16, u8)]) -> Vec<u8> {
    let mut bits = Vec::new();
    for &(code, width) in codes {
        bits.extend((0..width).rev().map(|bit| ((code >> bit) & 1) as u8));
    }
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0, |byte, (bit, value)| byte | (value << (7 - bit)))
        })
        .collect()
}

/// literalだけのcode番号から、仕様の三つの固定境界で幅を直接指定する。
fn literal_width(ordinal: usize, early: usize) -> u8 {
    match ordinal + early {
        0..=255 => 9,
        256..=767 => 10,
        768..=1791 => 11,
        _ => 12,
    }
}

/// EarlyChange両値の全幅変更・満杯の辞書・12bit clear後の9bit復帰を検査する。
#[test]
fn both_early_changes_keep_full_table_and_clear_boundaries() {
    let mut expected: Vec<u8> = (0..5000).map(|index| (index % 251) as u8).collect();
    expected.extend_from_slice(b"ABABA");
    for early in [0, 1] {
        let mut codes = vec![(256, 9)];
        codes
            .extend((0..5000).map(|index| ((index % 251) as u16, literal_width(index + 1, early))));
        codes.push((256, 12));
        codes.extend([(65, 9), (66, 9), (258, 9), (65, 9), (257, 9)]);
        let budget = DecodeBudget::default();
        assert_eq!(
            decode_lzw(&pack_widths(&codes), &budget, early as u8),
            Some(expected.clone())
        );
        assert!(budget.check().is_ok());
    }
}

/// parameterの既定値・明示値を検査し、未対応predictorを勝手に無視しない。
#[test]
fn decode_parameters_are_checked_without_coercion() {
    for (dictionary, expected) in [
        ("<< >>", Some(1)),
        ("<< /DecodeParms null >>", Some(1)),
        ("<< /DecodeParms << >> >>", Some(1)),
        (
            "<< /DecodeParms << /EarlyChange 0 /Predictor 1 >> >>",
            Some(0),
        ),
        ("<< /DecodeParms << /EarlyChange 1 >> >>", Some(1)),
        ("<< /DecodeParms << /EarlyChange 2 >> >>", None),
        ("<< /DecodeParms << /EarlyChange -1 >> >>", None),
        ("<< /DecodeParms << /EarlyChange 1.0 >> >>", None),
        ("<< /DecodeParms << /EarlyChange true >> >>", None),
        ("<< /DecodeParms << /EarlyChange 7 0 R >> >>", None),
        ("<< /DecodeParms << /Predictor 2 >> >>", None),
        ("<< /DecodeParms << /Predictor 12 >> >>", None),
        ("<< /DecodeParms [null] >>", None),
        ("<< /DecodeParms 7 0 R >>", None),
    ] {
        assert_eq!(
            early_change(dictionary.as_bytes()),
            expected,
            "{dictionary}"
        );
    }
}

/// 途中と開始前のキャンセルは、終端扱いや成功した再開へ変わらない。
#[test]
fn cancellation_stops_code_expansion_and_remains_terminal() {
    let data = nine_bit(&[256, 65, 258, 257]);
    let token = crate::ParseCancellation::new();
    let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    let mut decoder = Decoder::new(&data, 1, &budget).unwrap();
    let mut byte = [0];
    assert_eq!(decoder.read(&mut byte).unwrap(), 1);
    assert_eq!(byte, [b'A']);
    token.cancel();
    assert_eq!(
        decoder.read(&mut byte).unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
    assert!(decode_lzw(&data, &budget, 1).is_none());
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// KwKwK列の既知の総長で、単一streamと累積展開のinclusive境界を比較する。
    #[test]
    fn repetitive_dictionary_output_obeys_both_decoded_limits(count in 0u16..48) {
        let mut codes = vec![256, 65];
        codes.extend((0..count).map(|index| 258 + index));
        codes.push(257);
        let data = nine_bit(&codes);
        let length = 1 + u64::from(count) * (u64::from(count) + 3) / 2;
        let expected = vec![b'A'; length as usize];
        let exact = DecodeBudget::new(&ParseLimits { max_decoded_stream_bytes: length, max_total_decoded_bytes: length, ..Default::default() });
        prop_assert_eq!(decode_lzw(&data, &exact, 1), Some(expected));
        prop_assert!(exact.check().is_ok());
        prop_assert!(decode_lzw(&data, &exact, 1).is_none());
        prop_assert!(matches!(exact.check(), Err(ParseError::ResourceLimit { resource: "total decoded bytes", .. })), "再展開も累積する");
        let short = DecodeBudget::new(&ParseLimits { max_decoded_stream_bytes: length - 1, ..Default::default() });
        prop_assert!(decode_lzw(&data, &short, 1).is_none());
        prop_assert!(matches!(short.check(), Err(ParseError::ResourceLimit { resource: "decoded stream bytes", .. })), "単一streamの不足を拒否する");
    }

    /// 任意の破損bit列も、設定した展開量を越えて成功したりpanicしたりしない。
    #[test]
    fn arbitrary_codes_remain_bounded(data in proptest::collection::vec(any::<u8>(), 0..1024), limit in 0u64..2048, early in 0u8..2) {
        let budget = DecodeBudget::new(&ParseLimits { max_decoded_stream_bytes: limit, max_total_allocation_bytes: 65536, ..Default::default() });
        if let Some(output) = decode_lzw(&data, &budget, early) {
            prop_assert!(output.len() as u64 <= limit);
            prop_assert!(budget.check().is_ok());
        }
    }
}
