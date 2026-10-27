use super::validate_selections;
use drip_core::{
    SourcePath,
    protocol::{EntryKind, Selection},
};

fn selection(id: &str, active: bool) -> Selection {
    Selection {
        id: id.to_owned(),
        source: SourcePath::new('C', vec!["file.txt".to_owned()]).unwrap(),
        root_kind: EntryKind::File,
        active,
    }
}

#[test]
fn selection_ids_must_be_lower_hex() {
    assert!(validate_selections(&[selection("A".repeat(32).as_str(), true)]).is_err());
}

#[test]
fn selection_set_allows_independent_active_roots() {
    let first = selection(&"a".repeat(32), true);
    let mut second = selection(&"b".repeat(32), true);
    second.source = SourcePath::new('D', vec!["other.txt".to_owned()]).unwrap();
    assert!(validate_selections(&[first, second]).is_ok());
}

#[test]
fn selection_set_rejects_overlapping_active_roots() {
    let first = Selection {
        root_kind: EntryKind::Directory,
        ..selection(&"a".repeat(32), true)
    };
    let mut second = selection(&"b".repeat(32), true);
    second.source = SourcePath::new('c', vec!["file.txt".to_owned(), "child".to_owned()]).unwrap();
    assert!(validate_selections(&[first, second]).is_err());
}

#[test]
fn active_root_overlap_matches_simple_unicode_uppercase_rules() {
    let first = Selection {
        source: SourcePath::new('C', vec!["É".to_owned()]).unwrap(),
        root_kind: EntryKind::Directory,
        ..selection(&"a".repeat(32), true)
    };
    let mut accented_child = selection(&"b".repeat(32), true);
    accented_child.source = SourcePath::new('C', vec!["é".to_owned(), "child".to_owned()]).unwrap();
    assert!(validate_selections(&[first.clone(), accented_child]).is_err());

    let mut sharp_s = first;
    sharp_s.source = SourcePath::new('C', vec!["ß".to_owned()]).unwrap();
    let mut double_s_child = selection(&"c".repeat(32), true);
    double_s_child.source =
        SourcePath::new('C', vec!["SS".to_owned(), "child".to_owned()]).unwrap();
    assert!(validate_selections(&[sharp_s, double_s_child]).is_ok());
}
