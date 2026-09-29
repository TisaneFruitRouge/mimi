use super::*;

fn config() -> MatrixConfig {
    MatrixConfig {
        user_id: "@mimi:example.org".into(),
        homeserver: "https://matrix.example.org/".into(),
        device_id: "DEVICE".into(),
        access_token: "secret".into(),
        store_passphrase: "pass".into(),
        pairing_code: Some("123456".into()),
        owner: None,
        owner_name: None,
        room_id: None,
        conversation_id: None,
        encrypted: false,
        cross_signed: true,
        signed_out: false,
        since: 0,
    }
}

#[test]
fn accounts_are_read_from_what_the_user_typed() {
    assert_eq!(
        parse_account(" @mimi:example.org ", None).unwrap(),
        Account {
            login: "@mimi:example.org".into(),
            server: Server::Name("example.org".into())
        }
    );
    // Without the @.
    assert_eq!(
        parse_account("mimi:example.org", None).unwrap().login,
        "@mimi:example.org"
    );
    // A name plus the server's address.
    let account = parse_account("mimi", Some("https://matrix.example.org")).unwrap();
    assert_eq!(account.login, "mimi");
    assert_eq!(
        account.server,
        Server::Url(Url::parse("https://matrix.example.org").unwrap())
    );
    assert_eq!(
        parse_account("mimi", Some("example.org")).unwrap().server,
        Server::Name("example.org".into())
    );
    // A server on this computer may use http; others may not.
    assert!(parse_account("@mimi:localhost", Some("http://127.0.0.1:8008")).is_ok());
    assert!(
        parse_account("@mimi:example.org", Some("http://matrix.example.org"))
            .unwrap_err()
            .contains("https://")
    );
    assert!(parse_account("mimi", None).is_err());
    assert!(parse_account("@:example.org", None).is_err());
    assert!(parse_account("my assistant", Some("example.org")).is_err());
}

#[test]
fn only_the_code_pairs_and_then_only_the_owner_counts() {
    let mut c = config();
    assert_eq!(classify(&c, "!a:x", "@anyone:x"), Sender::Claimant);
    assert!(is_pairing_code(&c, " 123456\n"));
    assert!(!is_pairing_code(&c, "123457"));
    assert!(!is_pairing_code(&c, "my code is 123456"));
    assert!(accepts_invite(&c, Some("@anyone:x")));

    c.owner = Some("@me:x".into());
    c.room_id = Some("!dm:x".into());
    c.pairing_code = None;
    assert!(!is_pairing_code(&c, "123456"));
    assert_eq!(classify(&c, "!dm:x", "@me:x"), Sender::Owner);
    assert_eq!(classify(&c, "!other:x", "@me:x"), Sender::OwnerElsewhere);
    assert_eq!(classify(&c, "!dm:x", "@mallory:x"), Sender::Stranger);
    // One chat with the owner at a time: a new one only after they left theirs.
    assert!(!accepts_invite(&c, Some("@me:x")));
    assert!(!accepts_invite(&c, Some("@mallory:x")));
    assert!(!accepts_invite(&c, None));
    c.room_id = None;
    assert!(accepts_invite(&c, Some("@me:x")));
    assert!(!accepts_invite(&c, Some("@mallory:x")));
}

#[test]
fn an_encrypted_chat_only_takes_encrypted_messages() {
    let mut c = config();
    assert!(trusted(&c, false), "a plain chat takes plain messages");
    assert!(trusted(&c, true));
    c.encrypted = true;
    assert!(
        !trusted(&c, false),
        "a plain message in an encrypted chat is ignored"
    );
    assert!(trusted(&c, true));
}

#[test]
fn status_lines_say_what_to_do_and_how_private_it_is() {
    let mut c = config();
    let (status, detail, url) = describe(&c);
    assert_eq!(status, ConnectionStatus::NeedsAction);
    assert!(detail.contains("123456") && detail.contains("@mimi:example.org"));
    assert_eq!(
        url.as_deref(),
        Some("https://matrix.to/#/@mimi:example.org")
    );

    c.owner = Some("@me:x".into());
    c.owner_name = Some("Vincent".into());
    c.room_id = Some("!dm:x".into());
    c.encrypted = true;
    let (status, detail, _) = describe(&c);
    assert_eq!(status, ConnectionStatus::Ok);
    assert!(detail.contains("Vincent") && detail.contains("end-to-end encrypted"));
    assert!(!detail.contains("verified"));

    c.encrypted = false;
    assert!(describe(&c).1.contains("not encrypted"));

    c.room_id = None;
    assert_eq!(describe(&c).0, ConnectionStatus::NeedsAction);

    c.signed_out = true;
    assert_eq!(describe(&c).0, ConnectionStatus::Error);
}

#[test]
fn secrets_stay_out_of_what_clients_see() {
    let c = config();
    let (_, detail, url) = describe(&c);
    for text in [detail, url.unwrap_or_default()] {
        assert!(!text.contains("secret") && !text.contains("pass"), "{text}");
    }
}
