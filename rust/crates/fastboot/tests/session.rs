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
