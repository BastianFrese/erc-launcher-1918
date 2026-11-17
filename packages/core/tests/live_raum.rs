//! Temporaer: prueft den Launcher-Weg (REST + WebSocket) gegen den echten Raumserver.
use std::time::Duration;

use swofty_core::multiplayer::rooms::{Api, RoomSettings};
use swofty_core::multiplayer::ws::{Event, Socket};

/// `ERC_RAUM` overrides the address, e.g. for a tunnel to the container.
fn server() -> String {
    std::env::var("ERC_RAUM").unwrap_or_else(|_| "https://raum.erdi-erc.de".to_owned())
}

#[test]
#[ignore]
fn der_echte_raumserver_traegt_eine_lobby() {
    let server = server();
    let host = Api::with_server(&server, "live-probe-host").with_name("Basti Probe");
    let me = host.me().unwrap();
    assert_eq!(me.name, "Basti Probe");
    let lobby = host.create(&RoomSettings::default()).unwrap();
    assert_eq!(lobby.code.len(), 4);
    assert!(lobby.key.as_ref().and_then(|k| k.bytes()).is_some(), "32-Byte-Schluessel");
    assert_eq!(lobby.members.len(), 1);

    let guest = Api::with_server(&server, "live-probe-gast").with_name("Gast Probe");
    let joined = guest.join(&lobby.code).unwrap();
    assert_eq!(joined.members.len(), 2);

    let url = host.ws_url(&lobby.code).unwrap();
    println!("ws: {url}");
    let mut socket = Socket::connect(&url, "live-probe-host", Duration::from_secs(10)).unwrap();
    let mut gesehen = 0;
    let frist = std::time::Instant::now() + Duration::from_secs(10);
    let mut erste = false;
    while std::time::Instant::now() < frist && gesehen < 1 {
        match socket.read().unwrap() {
            Some(Event::Text(text)) => {
                println!("ws: {text}");
                assert!(text.contains("\"lobby\""), "{text}");
                erste = true;
                gesehen += 1;
            }
            Some(Event::Closed { code, reason }) => panic!("zugemacht: {code:?} {reason}"),
            None => {}
        }
    }
    assert!(erste, "kein Stand ueber den WebSocket");
    socket.close();

    guest.leave(&lobby.code).unwrap();
    host.leave(&lobby.code).unwrap();
    std::thread::sleep(Duration::from_millis(200));
}
