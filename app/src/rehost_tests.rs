//! Settings ▸ Session ▸ Port while hosting: the session moves live.

use std::net::{TcpListener, TcpStream};

fn cfg() -> crate::config::SessionConfig {
    let mut c = crate::config::SessionConfig::default();
    c.ensure_room_keys();
    c
}

/// A port nothing holds right now.
fn free_port() -> u16 {
    TcpListener::bind(("0.0.0.0", 0)).unwrap().local_addr().unwrap().port()
}

#[test]
fn rehost_moves_the_session_live_and_keeps_it_on_a_bad_port() {
    let c = cfg();
    let old = crate::start_host(&c, 0).expect("a session");
    let from = old.port;
    // Same port: nothing to do.
    assert!(crate::rehost(&old, from, |p| crate::start_host(&c, p)).is_none());
    // A taken port: an error, and the old session still takes guests.
    let squatter = TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let taken = squatter.local_addr().unwrap().port();
    let got = crate::rehost(&old, taken, |p| crate::start_host(&c, p));
    assert!(matches!(got, Some(Err(_))), "taken port refused");
    assert!(TcpStream::connect(("127.0.0.1", from)).is_ok(), "old session still up");
    // A free port: the new session is there, listening, before the old goes.
    let want = free_port();
    let new = match crate::rehost(&old, want, |p| crate::start_host(&c, p)) {
        Some(Ok(h)) => h,
        other => panic!("moved: {:?}", other.map(|r| r.map(|h| h.port))),
    };
    assert_eq!(new.port, want);
    assert!(TcpStream::connect(("127.0.0.1", want)).is_ok(), "new session takes guests");
    drop(old);
    drop(new);
}
