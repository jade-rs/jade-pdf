#[path = "support/ruled_grid_fixture.rs"]
mod fixture;

/// nativeでもブラウザーと同じ自作資料の期待を検証する。
#[test]
fn closed_cell_order_and_conservative_fallbacks_share_browser_gold() {
    assert_eq!(fixture::check().unwrap(), 109);
}
