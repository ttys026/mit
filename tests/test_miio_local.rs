use mit::miio_local::{build_handshake_packet, encrypt_payload, parse_token_hex};

#[test]
fn parse_token_hex_accepts_32_hex_chars() {
    let token = parse_token_hex("00112233445566778899aabbccddeeff").unwrap();
    assert_eq!(token.len(), 16);
    assert_eq!(token[0], 0x00);
    assert_eq!(token[15], 0xff);
}

#[test]
fn handshake_packet_has_expected_magic_and_length() {
    let packet = build_handshake_packet();
    assert_eq!(packet.len(), 32);
    assert_eq!(packet[0], 0x21);
    assert_eq!(packet[1], 0x31);
    assert_eq!(packet[2], 0x00);
    assert_eq!(packet[3], 0x20);
}

#[test]
fn payload_encryption_changes_plaintext() {
    let token = parse_token_hex("00112233445566778899aabbccddeeff").unwrap();
    let encrypted = encrypt_payload(&token, br#"{"id":1,"method":"miIO.info"}"#).unwrap();
    assert!(!encrypted.is_empty());
    assert_ne!(encrypted, br#"{"id":1,"method":"miIO.info"}"#.to_vec());
}
