use super::{fingerprint, slot_url, Account, Accounts};

#[test]
fn add_active_adds_unlabelled_account_and_makes_it_active() {
    let mut accounts = Accounts::default();
    let (id, new) = accounts.add_active("aaaa");
    assert!(new);
    assert_eq!(accounts.active, Some(id));
    assert_eq!(
        accounts.get(id),
        Some(&Account {
            id,
            owner: String::new(),
            key_name: String::new(),
            fingerprint: "aaaa".to_owned(),
        })
    );
}

#[test]
fn adding_same_fingerprint_again_returns_existing_id_and_activates_it() {
    let mut accounts = Accounts::default();
    let (first, _) = accounts.add_active("aaaa");
    let (other, _) = accounts.add_active("bbbb");
    assert_ne!(first, other);

    let (again, new) = accounts.add_active("aaaa");
    assert_eq!(again, first);
    assert!(!new);
    assert_eq!(accounts.active, Some(first));
    assert_eq!(accounts.accounts.len(), 2);
}

#[test]
fn ids_are_never_reused_after_remove() {
    let mut accounts = Accounts::default();
    let (a, _) = accounts.add_active("aaaa");
    let (b, _) = accounts.add_active("bbbb");
    assert!(accounts.remove(a).is_some());
    let (c, new) = accounts.add_active("cccc");
    assert!(new);
    assert_ne!(c, a);
    assert_ne!(c, b);
    assert!(c > b);
}

#[test]
fn removing_active_account_leaves_none_active() {
    let mut accounts = Accounts::default();
    let (id, _) = accounts.add_active("aaaa");
    assert!(accounts.remove(id).is_some());
    assert_eq!(accounts.active, None);
    assert!(accounts.get(id).is_none());
}

#[test]
fn set_active_on_missing_id_is_false_and_changes_nothing() {
    let mut accounts = Accounts::default();
    let (id, _) = accounts.add_active("aaaa");
    assert!(!accounts.set_active(id + 100));
    assert_eq!(accounts.active, Some(id));
}

#[test]
fn label_active_ignores_empty_strings() {
    let mut accounts = Accounts::default();
    let (id, _) = accounts.add_active("aaaa");
    assert!(accounts.label_active("Owner", "Key"));
    accounts.label_active("", "Renamed");
    let account = accounts.get(id).unwrap();
    assert_eq!(account.owner, "Owner");
    assert_eq!(account.key_name, "Renamed");
}

#[test]
fn json_round_trip_preserves_index() {
    let mut accounts = Accounts::default();
    accounts.add_active("aaaa");
    let (id, _) = accounts.add_active("bbbb");
    accounts.label_active("Owner", "Key");
    assert!(accounts.set_active(id));
    let restored = Accounts::from_json(&accounts.to_json());
    assert_eq!(restored, accounts);
}

#[test]
fn from_json_drops_active_naming_no_account() {
    let orphaned = Accounts {
        accounts: Vec::new(),
        active: Some(7),
        next_id: 8,
    };
    let restored = Accounts::from_json(&orphaned.to_json());
    assert_eq!(restored.active, None);
}

#[test]
fn from_json_malformed_text_gives_default() {
    assert_eq!(Accounts::from_json("not json {"), Accounts::default());
    assert_eq!(Accounts::from_json(""), Accounts::default());
}

#[test]
fn fingerprint_is_sixteen_lowercase_hex_digits() {
    let print = fingerprint("secret");
    assert_eq!(print.len(), 16);
    assert!(print
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
}

#[test]
fn fingerprint_is_stable_and_differs_for_different_secrets() {
    assert_eq!(fingerprint("secret"), fingerprint("secret"));
    assert_ne!(fingerprint("secret"), fingerprint("secret2"));
}

#[test]
fn slot_url_differs_per_id_and_contains_id() {
    let three = slot_url(3);
    assert_ne!(three, slot_url(4));
    assert!(three.contains('3'));
}
