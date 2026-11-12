//! A [`Transport`] over our own relay (`f1.erdi-erc.de:35501`).
//!
//! The relay forwards every datagram to the other members of the same room, so
//! nobody needs an open port. That replaces the TURN allocation the lobby server
//! would hand out; the packet layout is shared with `server/relay/erc-relay.py`:
//!
//! ```text
//! "ERC-RELAY" | room (4 ASCII) | sender (1 byte) | payload
//! ```
//!
//! The room code is the one players share, so a room needs no account anywhere.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::direct::timed_out;
use super::transport::{MAX_DATAGRAM, Transport};

const HEAD: &[u8] = b"ERC-RELAY";
const ROOM_LEN: usize = 4;
/// What the relay adds in front of a payload.
pub const HEADER: usize = HEAD.len() + ROOM_LEN + 1;

/// The relay only forwards to members it has heard from, and forgets a member
/// after ten silent minutes. This is what we say to be heard, and to stay known.
/// It never reaches the game: `unframe` drops it.
const HELLO: &[u8] = b"ERC-HELLO";
const HELLO_EVERY: Duration = Duration::from_secs(60);

/// How the host describes the room to the relay: `ERC-RAUM name=…;kat=…`.
const DESCRIBE: &[u8] = b"ERC-RAUM ";
/// The host's full mod fingerprint, sent in pieces: `ERC-FP` total seq payload.
const ABDRUCK: &[u8] = b"ERC-FP";
/// A guest asking for it.
const ABDRUCK_FRAGE: &[u8] = b"ERC-FP?";
/// Klopfen: der Gast fragt, ob der Weg zum Host wirklich steht.
const PING: &[u8] = b"ERC-PING";
/// Antwort des Hosts darauf.
const PONG: &[u8] = b"ERC-PONG";

/// The key every member of a room seals with. The room code is the shared secret
/// — whoever has the code can read the room, and nobody else can.
pub fn room_key(code: &str) -> [u8; 32] {
    Sha256::digest(format!("ERC-Lobby/{code}").as_bytes()).into()
}

/// A control packet, not game traffic: our own liveness, or the host's room
/// description. These never reach the bridge.
fn control(payload: &[u8]) -> bool {
    payload == HELLO
        || payload == PING
        || payload == PONG
        || payload.starts_with(DESCRIBE)
        || payload.starts_with(ABDRUCK)
}

/// The relay as deployed. `ERC_RELAY` overrides it, for a relay next to you.
pub const DEFAULT_RELAY: &str = "f1.erdi-erc.de:35501";

pub fn default_relay() -> String {
    std::env::var("ERC_RELAY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_RELAY.to_owned())
}

/// [`default_relay`] with its name resolved, for [`super::session::Relay::Own`].
pub fn default_relay_addr() -> io::Result<SocketAddr> {
    default_relay()
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the relay has no address"))
}

pub struct RelayUdp {
    socket: UdpSocket,
    relay: SocketAddr,
    room: [u8; ROOM_LEN],
    me: u8,
    members: RwLock<HashSet<u8>>,
    /// Everyone the relay has shown us in this room, whether the bridge knows them
    /// yet or not. The launcher polls this to add members as they turn up.
    seen: RwLock<BTreeSet<u8>>,
    unknown: AtomicU64,
    /// When we last told the relay we are here, in milliseconds since the epoch.
    heard_at: AtomicU64,
    /// The host's mod fingerprint as compressed JSON, sent to guests that ask.
    abdruck: RwLock<Option<Vec<u8>>>,
}

impl RelayUdp {
    /// `room` is the four characters players type; `me` this player's lobby number.
    pub fn new(relay: impl ToSocketAddrs, room: &str, me: u8) -> io::Result<Self> {
        let room = room_bytes(room)?;
        if !(1..=254).contains(&me) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "lobby members are numbered 1 to 254",
            ));
        }
        let relay = relay
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the relay has no address"))?;
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        Ok(Self {
            socket,
            relay,
            room,
            me,
            members: RwLock::default(),
            seen: RwLock::default(),
            unknown: AtomicU64::new(0),
            heard_at: AtomicU64::new(0),
            abdruck: RwLock::default(),
        })
    }

    /// Everyone this room has shown us, this player included.
    pub fn seen(&self) -> Vec<u8> {
        let mut all: BTreeSet<u8> = self
            .seen
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        all.insert(self.me);
        all.into_iter().collect()
    }

    /// Tells the relay what this room is called, so it shows up in the list.
    /// The host keeps its fingerprint here; guests ask for it before they join.
    /// Compressed, because a profile with a dozen addons makes several KB of JSON.
    pub fn set_abdruck(&self, json: Option<String>) {
        let packed = json.and_then(|text| {
            let mut out = Vec::new();
            let mut enc = flate2::write::ZlibEncoder::new(
                &mut out,
                flate2::Compression::fast(),
            );
            use std::io::Write;
            enc.write_all(text.as_bytes()).ok()?;
            enc.finish().ok()?;
            Some(out)
        });
        *self.abdruck.write().unwrap_or_else(|e| e.into_inner()) = packed;
    }

    /// Sends the stored fingerprint in as many pieces as it takes.
    fn sende_abdruck(&self) {
        let packet = self.abdruck.read().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(bytes) = packet else { return };
        let raum = MAX_DATAGRAM - HEADER - ABDRUCK.len() - 2;
        let stuecke = bytes.len().div_ceil(raum).min(255) as u8;
        for (i, teil) in bytes.chunks(raum).take(255).enumerate() {
            let mut payload = Vec::with_capacity(ABDRUCK.len() + 2 + teil.len());
            payload.extend_from_slice(ABDRUCK);
            payload.push(stuecke);
            payload.push(i as u8);
            payload.extend_from_slice(teil);
            let _ = self.socket.send_to(&self.frame(&payload), self.relay);
        }
    }

    /// As a guest: asks the host for its fingerprint and puts the pieces back
    /// together. `Ok(None)` when the host has none to give.
    pub fn hole_abdruck(&self, frist: Duration) -> io::Result<Option<Vec<u8>>> {
        let _ = self.socket.send_to(&self.frame(ABDRUCK_FRAGE), self.relay);
        let deadline = Instant::now() + frist;
        let mut packet = [0u8; MAX_DATAGRAM];
        let mut teile: Option<Vec<Option<Vec<u8>>>> = None;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return match teile {
                    Some(t) if t.iter().all(Option::is_some) => {
                        Ok(Some(t.into_iter().flatten().flatten().collect()))
                    }
                    Some(_) => Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "the fingerprint did not arrive in full",
                    )),
                    None => Ok(None),
                };
            }
            self.announce_if_due();
            self.socket
                .set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
            let n = match self.socket.recv_from(&mut packet) {
                Ok((n, from)) => {
                    if from.ip() != self.relay.ip() {
                        continue;
                    }
                    n
                }
                Err(e) if timed_out(&e) => continue,
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                Err(e) => return Err(e),
            };
            let Some((_, payload)) = self.unframe(&packet[..n]) else {
                continue;
            };
            if !payload.starts_with(ABDRUCK) || payload.starts_with(ABDRUCK_FRAGE) {
                continue;
            }
            if payload.len() < ABDRUCK.len() + 2 {
                continue;
            }
            let total = payload[ABDRUCK.len()] as usize;
            let seq = payload[ABDRUCK.len() + 1] as usize;
            if total == 0 || seq >= total {
                continue;
            }
            let stuecke = teile.get_or_insert_with(|| vec![None; total]);
            if stuecke.len() != total {
                continue;
            }
            if stuecke[seq].is_none() {
                stuecke[seq] = Some(payload[ABDRUCK.len() + 2..].to_vec());
            }
            if stuecke.iter().all(Option::is_some) {
                let ganz: Vec<u8> = stuecke.iter().flatten().flatten().copied().collect();
                let mut dec = flate2::read::ZlibDecoder::new(&ganz[..]);
                let mut aus = String::new();
                use std::io::Read;
                dec.read_to_string(&mut aus)?;
                return Ok(Some(aus.into_bytes()));
            }
        }
    }

    /// Tells the relay what this room is called, so it shows up in the list, and
    /// which mods the host plays (the fingerprint digest), so a guest can check
    /// before it joins.
    pub fn describe(&self, name: &str, category: &str, fingerprint: Option<&str>) {
        let name = name.replace([';', '\n', '\r'], " ");
        let mut payload = format!("ERC-RAUM name={name};kat={category}");
        if let Some(f) = fingerprint {
            payload.push_str(&format!(";fp={f}"));
        }
        let _ = self.socket.send_to(&self.frame(payload.as_bytes()), self.relay);
    }

    /// Waits for the host's room description and returns the fingerprint digest in
    /// it. `Ok(None)` when the host described the room without one.
    pub fn wait_for_host_abdruck(&self, frist: Duration) -> io::Result<Option<String>> {
        let deadline = Instant::now() + frist;
        let mut packet = [0u8; MAX_DATAGRAM];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the host did not describe its room",
                ));
            }
            self.announce_if_due();
            self.socket
                .set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
            let n = match self.socket.recv_from(&mut packet) {
                Ok((n, from)) => {
                    if from.ip() != self.relay.ip() {
                        continue;
                    }
                    n
                }
                Err(e) if timed_out(&e) => continue,
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                Err(e) => return Err(e),
            };
            let Some((_, payload)) = self.unframe(&packet[..n]) else {
                continue;
            };
            if !payload.starts_with(DESCRIBE) {
                continue;
            }
            let text = String::from_utf8_lossy(&payload[DESCRIBE.len()..]).into_owned();
            for stueck in text.split(';') {
                if let Some(fp) = stueck.trim().strip_prefix("fp=") {
                    return Ok(Some(fp.to_owned()));
                }
            }
            return Ok(None);
        }
    }

    /// Tells the relay we are in this room. Without it the relay never forwards
    /// anything to us — it only knows members it has heard from.
    pub fn announce(&self) {
        let _ = self.socket.send_to(&self.frame(HELLO), self.relay);
        self.heard_at.store(millis(), Ordering::Relaxed);
    }

    fn announce_if_due(&self) {
        let last = self.heard_at.load(Ordering::Relaxed);
        let now = millis();
        if now.saturating_sub(last) >= HELLO_EVERY.as_millis() as u64 {
            self.announce();
        }
    }

    /// Klopft beim Host, bis er antwortet. Beweist, dass der Weg in beide
    /// Richtungen offen ist - erst dann darf das Spiel beitreten.
    pub fn warte_auf_host(&self, frist: Duration) -> bool {
        let ende = Instant::now() + frist;
        let mut puffer = [0u8; MAX_DATAGRAM];
        loop {
            let _ = self.socket.send_to(&self.frame(PING), self.relay);
            let links = ende.saturating_duration_since(Instant::now());
            if links.is_zero() {
                return false;
            }
            self.socket
                .set_read_timeout(Some(links.min(Duration::from_millis(500))))
                .ok();
            match self.socket.recv_from(&mut puffer) {
                Ok((n, from)) => {
                    if from.ip() != self.relay.ip() {
                        continue;
                    }
                    if let Some((_, payload)) = self.unframe(&puffer[..n])
                        && payload == PONG
                    {
                        return true;
                    }
                }
                Err(e) if timed_out(&e) => {}
                Err(_) => return false,
            }
        }
    }

    /// The address every member sends to: the relay, which fans out from there.
    pub fn relay_addr(&self) -> SocketAddr {
        self.relay
    }

    pub fn room(&self) -> String {
        String::from_utf8_lossy(&self.room).into_owned()
    }

    pub fn self_member(&self) -> u8 {
        self.me
    }

    /// Datagrams dropped because they came from someone outside the room.
    pub fn unknown_dropped(&self) -> u64 {
        self.unknown.load(Ordering::Relaxed)
    }

    fn frame(&self, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + payload.len());
        out.extend_from_slice(HEAD);
        out.extend_from_slice(&self.room);
        out.push(self.me);
        out.extend_from_slice(payload);
        out
    }

    /// Room and sender of a datagram the relay passed on, when it is for this room
    /// and came from someone else.
    fn unframe<'a>(&self, packet: &'a [u8]) -> Option<(u8, &'a [u8])> {
        if packet.len() < HEADER || &packet[..HEAD.len()] != HEAD {
            return None;
        }
        let room = &packet[HEAD.len()..HEAD.len() + ROOM_LEN];
        if room != self.room {
            return None;
        }
        let sender = packet[HEAD.len() + ROOM_LEN];
        if sender == self.me {
            return None;
        }
        let payload = &packet[HEADER..];
        Some((sender, payload))
    }
}

fn millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Four ASCII letters or digits, as the relay accepts them.
pub fn room_bytes(room: &str) -> io::Result<[u8; ROOM_LEN]> {
    let room = room.trim();
    if room.len() != ROOM_LEN || !room.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a room code is four letters or digits",
        ));
    }
    let mut out = [0u8; ROOM_LEN];
    out.copy_from_slice(room.as_bytes());
    Ok(out)
}

impl Transport for RelayUdp {
    fn local_addr(&self) -> SocketAddr {
        self.relay
    }

    fn set_peers(&self, peers: &HashMap<u8, SocketAddr>) -> io::Result<()> {
        // The relay knows where everyone is; we only need to know who is in the
        // room, so a stranger's datagram never reaches the game.
        let members: HashSet<u8> = peers.keys().copied().collect();
        *self.members.write().unwrap_or_else(|e| e.into_inner()) = members;
        // Whoever is in the room now has to know we are too.
        self.announce();
        Ok(())
    }

    fn permitted(&self, member: u8) -> bool {
        self.members
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&member)
    }

    fn send(&self, to: u8, payload: &[u8]) -> io::Result<()> {
        if !self.permitted(to) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no address for member {to}"),
            ));
        }
        if payload.len() + HEADER > MAX_DATAGRAM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the datagram does not fit",
            ));
        }
        self.socket.send_to(&self.frame(payload), self.relay).map(|_| ())
    }

    fn recv(&self, buf: &mut [u8], timeout: Duration) -> io::Result<Option<(u8, usize)>> {
        let deadline = Instant::now() + timeout;
        let mut packet = [0u8; MAX_DATAGRAM];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.announce_if_due();
            self.socket
                .set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
            match self.socket.recv_from(&mut packet) {
                Ok((n, from)) => {
                    // Only the relay may talk to us.
                    if from.ip() != self.relay.ip() {
                        self.unknown.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    let Some((sender, payload)) = self.unframe(&packet[..n]) else {
                        continue;
                    };
                    // Whoever turns up teaches us the room: the launcher polls
                    // `seen` and hands new members to the bridge.
                    self.seen
                        .write()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(sender);
                    if control(payload) {
                        if payload == ABDRUCK_FRAGE {
                            self.sende_abdruck();
                        } else if payload == PING {
                            // Der Gast will wissen, ob der Weg steht: kurz antworten.
                            let _ = self.socket.send_to(&self.frame(PONG), self.relay);
                        }
                        continue;
                    }
                    if !self.permitted(sender) {
                        self.unknown.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    let take = payload.len().min(buf.len());
                    buf[..take].copy_from_slice(&payload[..take]);
                    return Ok(Some((sender, take)));
                }
                Err(e) if timed_out(&e) => return Ok(None),
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    /// A relay on a free port, doing what `erc-relay.py` does: fan a datagram out
    /// to the other members of its room.
    fn fake_relay() -> (SocketAddr, Arc<AtomicBool>) {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = socket.local_addr().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        thread::spawn(move || {
            let mut seen: HashMap<([u8; 4], u8), SocketAddr> = HashMap::new();
            let mut packet = [0u8; MAX_DATAGRAM];
            while !flag.load(Ordering::Relaxed) {
                let Ok((n, from)) = socket.recv_from(&mut packet) else {
                    continue;
                };
                if n < HEADER || &packet[..HEAD.len()] != HEAD {
                    continue;
                }
                let mut room = [0u8; ROOM_LEN];
                room.copy_from_slice(&packet[HEAD.len()..HEAD.len() + ROOM_LEN]);
                let sender = packet[HEAD.len() + ROOM_LEN];
                seen.insert((room, sender), from);
                for ((r, m), to) in seen.iter() {
                    if *r == room && *m != sender {
                        let _ = socket.send_to(&packet[..n], to);
                    }
                }
            }
        });
        (addr, stop)
    }

    #[test]
    fn a_room_code_is_four_letters_or_digits() {
        assert!(room_bytes("AB12").is_ok());
        assert!(room_bytes("ab12").is_ok());
        assert!(room_bytes("ABC").is_err());
        assert!(room_bytes("ABCDE").is_err());
        assert!(room_bytes("AB-1").is_err());
    }

    #[test]
    fn the_frame_carries_head_room_and_sender() {
        let t = RelayUdp::new("127.0.0.1:9", "AB12", 3).unwrap();
        let frame = t.frame(b"hallo");
        assert_eq!(&frame[..HEAD.len()], HEAD);
        assert_eq!(&frame[HEAD.len()..HEAD.len() + ROOM_LEN], b"AB12");
        assert_eq!(frame[HEAD.len() + ROOM_LEN], 3);
        assert_eq!(&frame[HEADER..], b"hallo");
        assert_eq!(t.unframe(&frame), None, "own datagrams are not relayed back");
    }

    #[test]
    fn datagrams_travel_through_a_relay() {
        let (relay, stop) = fake_relay();
        let a = RelayUdp::new(relay, "AB12", 1).unwrap();
        let b = RelayUdp::new(relay, "AB12", 2).unwrap();
        a.set_peers(&HashMap::from([(2, relay)])).unwrap();
        b.set_peers(&HashMap::from([(1, relay)])).unwrap();
        // Let the relay hear both of them.
        thread::sleep(Duration::from_millis(150));

        a.send(2, b"to two").unwrap();
        let mut buf = [0u8; MAX_DATAGRAM];
        assert_eq!(
            b.recv(&mut buf, Duration::from_secs(2)).unwrap(),
            Some((1, 6))
        );
        assert_eq!(&buf[..6], b"to two");

        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn a_stranger_in_the_room_never_reaches_the_game() {
        let (relay, stop) = fake_relay();
        let a = RelayUdp::new(relay, "AB12", 1).unwrap();
        let b = RelayUdp::new(relay, "AB12", 2).unwrap();
        // `b` knows only member 1; `a` is member 1, so this must pass …
        b.set_peers(&HashMap::from([(1, relay)])).unwrap();
        // … but a member the bridge never added must not.
        a.set_peers(&HashMap::from([(2, relay)])).unwrap();
        thread::sleep(Duration::from_millis(150));
        a.send(2, b"hello").unwrap();
        let mut buf = [0u8; MAX_DATAGRAM];
        assert_eq!(
            b.recv(&mut buf, Duration::from_secs(2)).unwrap(),
            Some((1, 5))
        );

        // Member 3 sends, but `b` does not know it: dropped.
        let c = RelayUdp::new(relay, "AB12", 3).unwrap();
        c.set_peers(&HashMap::from([(1, relay), (2, relay)])).unwrap();
        thread::sleep(Duration::from_millis(150));
        c.send(1, b"stranger").unwrap();
        assert_eq!(
            b.recv(&mut buf, Duration::from_millis(300)).unwrap(),
            None
        );
        assert_eq!(b.unknown_dropped(), 1);

        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn sending_to_a_member_that_is_not_there_fails() {
        let t = RelayUdp::new("127.0.0.1:9", "AB12", 1).unwrap();
        assert!(t.send(2, b"x").is_err());
    }

    /// Live gegen den echten Relay: zwei Mitglieder, ein Paket hin und zurueck.
    ///
    ///     cargo test -p swofty-core --lib erreicht_den_echten_relay -- --ignored
    ///
    /// Achtung: der Raum bleibt zehn Minuten in der Lobbyliste stehen.
    #[test]
    #[ignore]
    fn erreicht_den_echten_relay() {
        let relay = default_relay();
        let a = RelayUdp::new(relay.as_str(), "TEST", 1).unwrap();
        let b = RelayUdp::new(relay.as_str(), "TEST", 2).unwrap();
        a.set_peers(&HashMap::from([(2, a.relay_addr())])).unwrap();
        b.set_peers(&HashMap::from([(1, b.relay_addr())])).unwrap();
        thread::sleep(Duration::from_millis(500));

        a.send(2, b"ueber den echten Relay").unwrap();
        let mut buf = [0u8; MAX_DATAGRAM];
        let received = b.recv(&mut buf, Duration::from_secs(3)).unwrap();
        assert_eq!(received.map(|(m, _)| m), Some(1), "kein Paket von Mitglied 1");
        assert_eq!(&buf[..22], b"ueber den echten Relay");

        // Und die Liste kennt den Raum jetzt.
        let list = UdpSocket::bind("0.0.0.0:0").unwrap();
        list.set_read_timeout(Some(Duration::from_secs(3))).ok();
        list.send_to(b"ERC-LOBBYS", a.relay_addr()).unwrap();
        let mut antwort = [0u8; 8192];
        let (n, _) = list.recv_from(&mut antwort).unwrap();
        let text = String::from_utf8_lossy(&antwort[..n]);
        assert!(text.contains("TEST"), "Raum fehlt in der Liste: {text}");
    }
}
