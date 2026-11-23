use super::*;
use std::collections::HashSet;

#[test]
fn every_limit_error_has_words_of_its_own() {
    assert!(LimitError::ALL.len() >= 52, "{}", LimitError::ALL.len());
    let texts: HashSet<&str> = LimitError::ALL.iter().map(LimitError::text).collect();
    assert_eq!(texts.len(), LimitError::ALL.len(), "two share their words");
    assert!(texts.iter().all(|t| !t.is_empty() && t.trim() == *t));
    for error in LimitError::ALL {
        assert_eq!(error.to_string(), error.text());
    }
}
