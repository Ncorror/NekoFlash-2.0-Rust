use nekoflash_adb::{
    connection::{Connection, HostIdentity},
    packet_io::{Link, UsbLink, MAX_PAYLOAD},
    services,
    wire::Header,
    VERSION_WITH_CHECKSUM as V,
};
use nekoflash_transport::Packets;
use std::cell::RefCell;
use std::io::{self, Cursor, Read, Write};
use std::num::NonZeroU16;
use std::rc::Rc;
use std::time::{Duration, Instant};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}
fn packet(cmd: &[u8; 4], a: u32, b: u32, body: &[u8]) -> Vec<u8> {
    let h = Header::for_payload(u32::from_le_bytes(*cmd), a, b, body, MAX_PAYLOAD).unwrap();
    [h.encode().as_slice(), body].concat()
}
struct Replay {
    input: Cursor<Vec<u8>>,
    writes: Rc<RefCell<Vec<Vec<u8>>>>,
}
impl Replay {
    fn new(messages: Vec<Vec<u8>>) -> Self {
        Self {
            input: Cursor::new(messages.concat()),
            writes: Rc::default(),
        }
    }
}
impl Link for Replay {
    fn read_exact(&mut self, buf: &mut [u8], _: Instant) -> io::Result<()> {
        self.input.read_exact(buf)
    }
    fn write_transfer(&mut self, b: &[u8], _: Instant) -> io::Result<()> {
        self.writes.borrow_mut().push(b.to_vec());
        Ok(())
    }
}
struct Identity;
impl HostIdentity for Identity {
    fn sign_token(&mut self, token: &[u8; 20]) -> io::Result<Vec<u8>> {
        assert_eq!(token, &[1; 20]);
        Ok(vec![8; 256])
    }
    fn public_key_payload(&mut self) -> io::Result<Vec<u8>> {
        Ok(b"fixture-key\0".to_vec())
    }
}
fn banner() -> Vec<u8> {
    packet(b"CNXN", V, 4096, b"device::features=shell_v2\0")
}
fn okay() -> Vec<u8> {
    packet(b"OKAY", 9, 1, b"")
}
fn write(body: &[u8]) -> Vec<u8> {
    packet(b"WRTE", 9, 1, body)
}
fn close() -> Vec<u8> {
    packet(b"CLSE", 9, 1, b"")
}

#[test]
fn classic_auth_binary_shell_and_early_remote_close() {
    let link = Replay::new(vec![
        packet(b"AUTH", 1, 0, &[1; 20]),
        packet(b"AUTH", 1, 0, &[2; 20]),
        banner(),
        okay(),
        write(b"\x01\x02\0\0\0\0\xff\x02\x01\0\0\0e\x03\x01\0\0\0\x07"),
        close(),
    ]);
    let writes = link.writes.clone();
    let mut c = Connection::usb(link, &mut Identity, deadline()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(c.shell_v2("id", deadline(), &mut out, &mut err).unwrap(), 7);
    assert_eq!(out, b"\0\xff");
    assert_eq!(err, b"e");
    assert!(writes.borrow().contains(&vec![8; 256]));
    assert!(writes.borrow().contains(&b"fixture-key\0".to_vec()));
}
#[test]
fn wireless_requires_upgrade_before_services() {
    let c = Connection::<Replay>::wireless(
        Replay::new(vec![banner()]),
        |_, _| panic!("downgrade accepted"),
        deadline(),
    );
    assert!(c.is_err());
    let c = Connection::wireless(
        Replay::new(vec![packet(b"STLS", V, 0, b"")]),
        |plain, _| {
            assert!(plain.writes.borrow().iter().any(|h| h.starts_with(b"STLS")));
            Ok(Replay::new(vec![banner()]))
        },
        deadline(),
    )
    .unwrap();
    assert_eq!(c.banner(), b"device::features=shell_v2\0");
}
#[test]
fn stale_stream_id_and_missing_exit_fail() {
    for reply in [packet(b"WRTE", 9, 2, b"bad"), close()] {
        let mut c = Connection::usb(
            Replay::new(vec![banner(), okay(), okay(), reply]),
            &mut Identity,
            deadline(),
        )
        .unwrap();
        assert!(c
            .shell_v2("date", deadline(), &mut Vec::new(), &mut Vec::new())
            .is_err());
    }
}
#[test]
fn sideload_retries_last_short_block_and_requires_done() {
    let mut c = Connection::usb(
        Replay::new(vec![
            banner(),
            okay(),
            write(b"00000001"),
            okay(),
            write(b"00000001"),
            okay(),
            write(b"DONEDONE"),
            close(),
        ]),
        &mut Identity,
        deadline(),
    )
    .unwrap();
    let mut data = Cursor::new(vec![1; 65539]);
    let mut progress = Vec::new();
    c.sideload_file(&mut data, 65539, deadline(), |n| {
        progress.push(n);
        Ok(())
    })
    .unwrap();
    assert_eq!(progress, [3, 3]);
    for reply in [write(b"00000002"), write(b"FAILFAIL"), close()] {
        let mut c = Connection::usb(
            Replay::new(vec![banner(), okay(), reply]),
            &mut Identity,
            deadline(),
        )
        .unwrap();
        assert!(c
            .sideload_file(&mut data, 65539, deadline(), |_| Ok(()))
            .is_err());
    }
}
struct Duplex {
    input: Cursor<Vec<u8>>,
    out: Vec<u8>,
}
impl Read for Duplex {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let n = b.len().min(3);
        self.input.read(&mut b[..n])
    }
}
impl Write for Duplex {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.out.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn sync_data_is_binary_and_confirmed() {
    let mut s = Duplex {
        input: Cursor::new(b"OKAY\0\0\0\0".to_vec()),
        out: Vec::new(),
    };
    services::push(&mut s, "/sdcard/item", &mut &b"\0\xff"[..], 2, 17, |_| {
        Ok(())
    })
    .unwrap();
    assert!(s.out.ends_with(b"DATA\x02\0\0\0\0\xffDONE\x11\0\0\0"));
    s.input = Cursor::new(b"DATA\x02\0\0\0\0\xffDONE\0\0\0\0".to_vec());
    let mut out = Vec::new();
    assert_eq!(
        services::pull(&mut s, "/sdcard/item", &mut out, |_| Ok(())).unwrap(),
        2
    );
    assert_eq!(out, b"\0\xff");
    s.input = Cursor::new(b"FAIL\x06\0\0\0denied".to_vec());
    assert!(services::pull(&mut s, "/sdcard/item", &mut out, |_| Ok(())).is_err());
}
#[test]
fn adb_usb_exact_multiple_ends_in_one_zlp() {
    struct Bulk(Rc<RefCell<Vec<usize>>>);
    impl Packets for Bulk {
        fn send(&mut self, b: &[u8], _: Instant) -> io::Result<()> {
            self.0.borrow_mut().push(b.len());
            Ok(())
        }
        fn receive(&mut self, _: &mut [u8], _: Instant) -> io::Result<usize> {
            unreachable!()
        }
    }
    let sizes = Rc::default();
    let mut link = UsbLink::new(Bulk(Rc::clone(&sizes)), NonZeroU16::new(512).unwrap()).unwrap();
    link.write_transfer(&[0; 32768], deadline()).unwrap();
    link.write_transfer(&[0; 24], deadline()).unwrap();
    assert_eq!(*sizes.borrow(), [16384, 16384, 0, 24]);
}

#[test]
fn drains_independent_stdout_packets_before_close_stdin_ack() {
    let link = Replay::new(vec![
        banner(),
        okay(),
        write(b"\x01\x01\0\0\0a"),
        write(b"\x01\x01\0\0\0b"),
        write(b"\x03\x01\0\0\0\0"),
        okay(),
        close(),
    ]);
    let mut c = Connection::usb(link, &mut Identity, deadline()).unwrap();
    let mut out = Vec::new();
    assert_eq!(
        c.shell_v2("cmd", deadline(), &mut out, &mut Vec::new())
            .unwrap(),
        0
    );
    assert_eq!(out, b"ab");
}

#[test]
fn wireless_classic_peer_requires_a_valid_banner_checksum() {
    let mut bad = banner();
    bad[16..20].fill(0);
    let plain = Replay::new(vec![packet(b"STLS", V, 0, b"")]);
    assert!(Connection::wireless(plain, |_, _| Ok(Replay::new(vec![bad])), deadline()).is_err());
}
