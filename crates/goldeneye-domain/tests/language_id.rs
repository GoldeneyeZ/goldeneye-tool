use std::collections::{BTreeSet, HashSet};

use goldeneye_domain::{LanguageId, LanguageIdError};

#[test]
fn language_id_preserves_non_empty_values_and_rejects_empty_values() {
    let language = LanguageId::new("rust").expect("rust is a valid language ID");

    assert_eq!(language.as_str(), "rust");
    assert_eq!(LanguageId::new(""), Err(LanguageIdError::Empty));
}

#[test]
fn language_id_has_value_ordering_and_hash_semantics() {
    let rust = LanguageId::new("rust").unwrap();
    let duplicate_rust = LanguageId::new("rust").unwrap();
    let python = LanguageId::new("python").unwrap();

    let hash_ids = HashSet::from([rust.clone(), duplicate_rust]);
    assert_eq!(hash_ids.len(), 1);

    let ordered_ids = BTreeSet::from([rust, python]);
    assert_eq!(
        ordered_ids
            .iter()
            .map(LanguageId::as_str)
            .collect::<Vec<_>>(),
        ["python", "rust"]
    );
}
