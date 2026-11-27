use nekoflash_fastboot::session::{Error, Session, TcpPackets};
use nekoflash_transport::{Packets, Tcp};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::{
    io::{self, Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

#[test]
fn fragmented_tcp_getvar_and_cancellation() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let peer = thread::spawn(move || {
        let (mut socket, _) = server.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut handshake = [0; 4];
        socket.read_exact(&mut handshake).unwrap();
        assert_eq!(&handshake, b"FB01");
        socket.write_all(b"FB02").unwrap();
        let mut buf = [0; 20];
        socket.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..8], &12u64.to_be_bytes());
        assert_eq!(&buf[8..], b"getvar:model");
        for reply in [&b"INFOmodel: neko"[..], b"OKAY"] {
            for b in (reply.len() as u64).to_be_bytes().iter().chain(reply) {
                socket.write_all(&[*b]).unwrap();
            }
        }
        assert_eq!(socket.read(&mut buf).unwrap(), 0);
    });
    let tcp = Tcp::connect(address, deadline()).unwrap();
    let cancel = tcp.cancellation_handle().unwrap();
    let mut session = Session::new(TcpPackets::handshake(tcp, deadline()).unwrap());
    assert_eq!(session.getvar("model", deadline(), |_| {}).unwrap(), "neko");
    cancel.cancel().unwrap();
    peer.join().unwrap();
}

struct Recorded {
    replies: VecDeque<&'static [u8]>,
    writes: Rc<RefCell<Vec<Vec<u8>>>>,
}
impl Packets for Recorded {
    fn send(&mut self, b: &[u8], _: Instant) -> io::Result<()> {
        self.writes.borrow_mut().push(b.to_vec());
        Ok(())
    }
    fn receive(&mut self, b: &mut [u8], _: Instant) -> io::Result<usize> {
        let p = self
            .replies
            .pop_front()
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        b[..p.len()].copy_from_slice(p);
        Ok(p.len())
    }
}
#[test]
fn flash_requires_matching_data_and_two_final_confirmations() {
    for (replies, expected, sent) in [
        (vec![&b"DATA00000002"[..]], false, 1),
        (vec![b"DATA00000003", b"FAILbad data"], false, 2),
        (vec![b"DATA00000003", b"O\0KAY"], false, 2),
        (vec![b"DATA00000003", b"OKAY", b"FAILlocked"], false, 3),
        (vec![b"DATA00000003", b"OKAY", b"OKAY"], true, 3),
    ] {
        let writes = Rc::new(RefCell::new(Vec::new()));
        let mut s = Session::new(Recorded {
            replies: replies.into(),
            writes: writes.clone(),
        });
        let result = s.flash("boot_a", &mut &b"img"[..], 3, Some(4), deadline(), |_| {
            Ok(())
        });
        assert_eq!(result.is_ok(), expected);
        let actual = writes.borrow();
        assert_eq!(actual.len(), sent);
        assert_eq!(actual[0], b"download:00000003");
        if sent == 3 {
            assert_eq!(actual[2], b"flash:boot_a");
        }
        if matches!(result, Err(Error::Transport(_))) {
            assert!(
                matches!(s.getvar("model",deadline(),|_|{}),Err(Error::Transport(e)) if e.kind()==io::ErrorKind::NotConnected)
            );
        }
    }
}

#[test]
fn stale_cancel_handle_does_not_hold_a_failed_connection_open() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let watcher = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut handshake = [0; 4];
        sock.read_exact(&mut handshake).unwrap();
        sock.write_all(b"FB00").unwrap();
        assert_eq!(sock.read(&mut handshake).unwrap(), 0);
    });
    let tcp = Tcp::connect(addr, deadline()).unwrap();
    let cancel = tcp.cancellation_handle().unwrap();
    assert!(TcpPackets::handshake(tcp, deadline()).is_err());
    watcher.join().unwrap();
    cancel.cancel().unwrap();
}

#[test]
fn scalar_getvar_preserves_colons_and_prefers_the_requested_info_key() {
    let cases: &[(&str, &[&[u8]], &str)] = &[
        ("serialno", &[b"OKAYaa:bb:cc:dd:ee:ff"], "aa:bb:cc:dd:ee:ff"),
        ("mac-address", &[b"OKAYMAC_ADDRESS: aa:bb"], "aa:bb"),
        (
            "partition-size:boot_a",
            &[
                b"INFO(bootloader) partition-size:boot_a: 0x10",
                b"INFOpartition-size:boot_b: 0x20",
                b"TEXTfinished reading",
                b"OKAY",
            ],
            "0x10",
        ),
        ("current-slot", &[b"INFOslot-count:2", b"OKAY"], ""),
        ("product", &[b"INFOmodel-x", b"OKAY"], "model-x"),
    ];
    for &(name, packets, expected) in cases {
        let mut session = Session::new(Recorded {
            replies: packets.iter().copied().collect(),
            writes: Rc::default(),
        });
        assert_eq!(session.getvar(name, deadline(), |_| {}).unwrap(), expected);
    }
}

#[test]
fn fetch_range_streams_large_tcp_frames_and_keeps_the_next_status_intact() {
    let data: Vec<_> = (0..70_001u32).map(|i| (i % 251) as u8).collect();
    let size = data.len() as u32;
    let expected_data = data.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let peer = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut hello = [0; 4];
        sock.read_exact(&mut hello).unwrap();
        assert_eq!(&hello, b"FB01");
        sock.write_all(b"FB01").unwrap();
        let read_request = |s: &mut std::net::TcpStream, expected: &[u8]| {
            let mut length = [0; 8];
            s.read_exact(&mut length).unwrap();
            assert_eq!(u64::from_be_bytes(length), expected.len() as u64);
            let mut body = vec![0; expected.len()];
            s.read_exact(&mut body).unwrap();
            assert_eq!(body, expected);
        };
        let send_frame = |s: &mut std::net::TcpStream, body: &[u8]| {
            s.write_all(&(body.len() as u64).to_be_bytes()).unwrap();
            for bytes in body.chunks(1771) {
                s.write_all(bytes).unwrap();
            }
        };
        read_request(&mut sock, b"fetch:vendor_boot_b:0x100000002:0x00011171");
        send_frame(&mut sock, b"INFOreading partition");
        send_frame(&mut sock, format!("DATA{size:08X}").as_bytes());
        send_frame(&mut sock, &data[..3]);
        send_frame(&mut sock, &data[3..]);
        send_frame(&mut sock, b"TEXTdone");
        send_frame(&mut sock, b"OKAY");
        read_request(&mut sock, b"getvar:product");
        send_frame(&mut sock, b"OKAYcoral");
    });
    let tcp = Tcp::connect(address, deadline()).unwrap();
    let mut session = Session::new(TcpPackets::handshake(tcp, deadline()).unwrap());
    let mut output = Vec::new();
    let mut last = 0;
    session
        .fetch_range(
            "vendor_boot_b",
            0x1_0000_0002,
            size,
            &mut output,
            deadline(),
            |count| {
                assert!(count >= last && count - last <= 16 * 1024);
                last = count;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(last, size);
    assert_eq!(output, expected_data);
    assert_eq!(
        session.getvar("product", deadline(), |_| {}).unwrap(),
        "coral"
    );
    peer.join().unwrap();
}

#[test]
fn fetch_range_requires_exact_data_and_a_terminal_acknowledgement() {
    type FetchCase<'a> = (&'a [&'a [u8]], &'a [u8], bool);
    let cases: &[FetchCase<'_>] = &[
        (&[b"DATA00000002"], b"", false),
        (&[b"DATA 00000003"], b"", false),
        (&[b"OKAY"], b"", false),
        (&[b"DATA00000003", b""], b"", false),
        (&[b"DATA00000003", b"a"], b"a", false),
        (&[b"DATA00000003", b"abc", b"DATA00000003"], b"abc", false),
        (&[b"FAILdenied"], b"", true),
        (
            &[b"DATA00000003", b"a", b"bc", b"FAILread error"],
            b"abc",
            true,
        ),
    ];
    for &(frames, partial, explicit_fail) in cases {
        let mut replies: VecDeque<_> = frames.iter().copied().collect();
        if explicit_fail {
            replies.push_back(b"OKAYready");
        }
        let mut s = Session::new(Recorded {
            replies,
            writes: Rc::default(),
        });
        let mut out = Vec::new();
        let result = s.fetch_range("boot", 0, 3, &mut out, deadline(), |_| Ok(()));
        assert!(result.is_err());
        assert_eq!(matches!(result, Err(Error::Device(_))), explicit_fail);
        assert_eq!(out, partial);
        let next = s.getvar("product", deadline(), |_| {});
        if explicit_fail {
            assert_eq!(next.unwrap(), "ready");
        } else {
            assert!(
                matches!(next, Err(Error::Transport(e)) if e.kind()==io::ErrorKind::NotConnected)
            );
        }
    }
}

#[test]
fn invalid_fetch_ranges_and_early_cancellation_send_no_command() {
    let writes = Rc::new(RefCell::new(Vec::new()));
    let mut session = Session::new(Recorded {
        replies: VecDeque::from([&b"OKAYready"[..]]),
        writes: writes.clone(),
    });
    for (name, offset, size) in [
        ("boot", 0, 0),
        ("boot", i64::MAX as u64, 1),
        ("bad:name", 0, 1),
        ("", 0, 1),
        (
            "this_partition_name_is_too_long_for_the_fetch_command",
            0,
            1,
        ),
    ] {
        assert!(
            matches!(session.fetch_range(name,offset,size,&mut Vec::new(),deadline(),|_| Ok(())),
            Err(Error::Transport(e)) if e.kind()==io::ErrorKind::InvalidInput)
        );
    }
    assert!(session
        .fetch_range("boot", 0, 1, &mut Vec::new(), deadline(), |_| Err(
            io::ErrorKind::Interrupted.into()
        ))
        .is_err());
    assert!(writes.borrow().is_empty());
    assert_eq!(
        session.getvar("product", deadline(), |_| {}).unwrap(),
        "ready"
    );
}

#[test]
fn cancelled_fetch_or_failed_destination_drops_the_partial_transaction() {
    struct FailedWriter;
    impl Write for FailedWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("destination full"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    for cancel in [true, false] {
        let writes = Rc::new(RefCell::new(Vec::new()));
        let mut s = Session::new(Recorded {
            replies: [&b"DATA00000003"[..], b"a", b"bc", b"OKAY"].into(),
            writes: writes.clone(),
        });
        let result = if cancel {
            let mut partial = Vec::new();
            let result = s.fetch_range("boot_a", 0, 3, &mut partial, deadline(), |done| {
                if done == 0 {
                    Ok(())
                } else {
                    Err(io::ErrorKind::Interrupted.into())
                }
            });
            assert_eq!(partial, b"a");
            result
        } else {
            s.fetch_range("boot_a", 0, 3, &mut FailedWriter, deadline(), |_| Ok(()))
        };
        assert!(matches!(result, Err(Error::Transport(_))));
        assert!(
            matches!(s.getvar("product", deadline(), |_| {}), Err(Error::Transport(e)) if e.kind()==io::ErrorKind::NotConnected)
        );
        assert_eq!(writes.borrow().len(), 1);
    }
}

#[test]
fn fetch_range_rejects_oversized_or_empty_tcp_frames_before_writing() {
    for length in [0u64, 4, u64::MAX] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let peer = thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut hello = [0; 4];
            sock.read_exact(&mut hello).unwrap();
            assert_eq!(&hello, b"FB01");
            sock.write_all(b"FB01").unwrap();
            let cmd = b"fetch:boot:0x00000000:0x00000003";
            let mut request = vec![0; 8 + cmd.len()];
            sock.read_exact(&mut request).unwrap();
            assert_eq!(&request[..8], &(cmd.len() as u64).to_be_bytes());
            assert_eq!(&request[8..], cmd);
            sock.write_all(&12u64.to_be_bytes()).unwrap();
            sock.write_all(b"DATA00000003").unwrap();
            sock.write_all(&length.to_be_bytes()).unwrap();
            assert_eq!(sock.read(&mut hello).unwrap(), 0);
        });
        let tcp = Tcp::connect(address, deadline()).unwrap();
        let mut s = Session::new(TcpPackets::handshake(tcp, deadline()).unwrap());
        let mut output = Vec::new();
        assert!(
            matches!(s.fetch_range("boot",0,3,&mut output,deadline(),|_| Ok(())),Err(Error::Transport(e)) if e.kind()==io::ErrorKind::InvalidData)
        );
        assert!(output.is_empty());
        peer.join().unwrap();
    }
}
