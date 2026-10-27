#[test]
fn shared_spelling_comparison_matches_native_windows_for_unicode_case_pairs() {
    let mut mismatches = Vec::new();
    for (left, right) in [
        ("é", "É"),
        ("ǅ", "ǆ"),
        ("ᾀ", "ᾈ"),
        ("ß", "ẞ"),
        ("straße", "STRASSE"),
        ("ﬀ", "ff"),
        ("İ", "i"),
        ("ı", "I"),
        ("K", "k"),
        ("σ", "ς"),
        ("𐐀", "𐐨"),
    ] {
        let left_path = drip_core::SourcePath::new('C', vec![left.into()]).unwrap();
        let right_path = drip_core::SourcePath::new('C', vec![right.into()]).unwrap();
        let shared =
            left_path.has_windows_prefix(&right_path) && right_path.has_windows_prefix(&left_path);
        let native = crate::source::same_path(&left_path, &right_path);
        println!("{left} versus {right}: shared={shared}, native={native}");
        if shared != native {
            mismatches.push((left, right));
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:?}");
}
