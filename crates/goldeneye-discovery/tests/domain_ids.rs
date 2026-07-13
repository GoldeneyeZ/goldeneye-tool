use goldeneye_discovery::LanguageId;
use goldeneye_domain::{LanguageId as DomainLanguageId, LanguageIdError};

fn accepts_domain_language_id(_: DomainLanguageId) {}

#[test]
fn discovery_reexports_the_domain_language_id_type() {
    let discovery_id = LanguageId::new("rust").unwrap();

    accepts_domain_language_id(discovery_id.clone());
    let domain_id: DomainLanguageId = discovery_id;
    assert_eq!(domain_id.as_str(), "rust");
}

#[test]
fn discovery_exposes_the_domain_validation_error() {
    assert_eq!(LanguageId::new(""), Err(LanguageIdError::Empty));
}
