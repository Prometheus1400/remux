#[test]
fn derive_handle_ui_tests() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/derive_named_enum.rs");
    t.pass("tests/ui/derive_tuple_and_unit_variants.rs");
    t.compile_fail("tests/ui/derive_rejects_struct.rs");
}
