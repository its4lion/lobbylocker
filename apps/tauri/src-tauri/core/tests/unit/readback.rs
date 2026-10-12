//! Unit tests for the readback module.
use super::*;
#[test]
fn only_local_lobbylocker_pipe_names_are_accepted() {
    assert!(validate_name(&format!("{PREFIX}{}", uuid::Uuid::new_v4().simple())).is_ok());
    for name in [
        r"C:\any-file",
        r"\\server\pipe\LobbyLocker.readback.1234",
        r"\\.\pipe\Other",
        PREFIX,
    ] {
        assert!(validate_name(name).is_err());
    }
}
#[test]
fn bounded_local_readback_round_trip() {
    let receiver = Receiver::new().unwrap();
    let name = receiver.name().to_owned();
    let sender = thread::spawn(move || send(&name, b"fixture only").unwrap());
    sender.join().unwrap();
    let (pid, bytes) = receiver.finish().unwrap();
    assert_eq!(pid, std::process::id());
    assert_eq!(bytes, b"fixture only");
}
#[test]
fn cancelled_authentication_releases_reader_without_connecting() {
    assert!(Receiver::new().unwrap().finish().is_err());
}
