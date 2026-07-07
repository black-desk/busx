mod common;

#[test]
fn fixture_starts_a_bus() {
    let b = common::bus();
    assert!(b.address.starts_with("unix:"), "address was: {}", b.address);
}
