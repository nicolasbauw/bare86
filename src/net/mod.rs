// SPDX-License-Identifier: GPL-3.0-or-later
//! Réseau en espace utilisateur (« user networking » de QEMU) : libslirp
//! tourne dans son propre thread et fait le NAT des trames de l'invité vers
//! les sockets de l'hôte. Réseau 10.0.2.0/24 : invité 10.0.2.15 (DHCP),
//! passerelle 10.0.2.2, DNS 10.0.2.3. Aucun droit particulier requis.

use std::collections::VecDeque;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[repr(C)]
struct RawNet {
    _private: [u8; 0],
}

type RxFn = extern "C" fn(user: *mut libc::c_void, frame: *const u8, len: usize);

unsafe extern "C" {
    fn bare86_net_new(rx: RxFn, user: *mut libc::c_void) -> *mut RawNet;
    fn bare86_net_input(n: *mut RawNet, frame: *const u8, len: usize);
    fn bare86_net_hostfwd(n: *mut RawNet, udp: libc::c_int, host_port: libc::c_int, guest_port: libc::c_int) -> libc::c_int;
    fn bare86_net_poll(n: *mut RawNet, wake_fd: libc::c_int, max_ms: libc::c_int) -> libc::c_int;
    fn bare86_net_free(n: *mut RawNet);
}

/// Redirection d'un port de l'hôte (127.0.0.1) vers l'invité.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forward {
    pub udp: bool,
    pub host: u16,
    pub guest: u16,
}

impl std::str::FromStr for Forward {
    type Err = String;

    /// `tcp:HÔTE:INVITÉ` ou `udp:HÔTE:INVITÉ` (`tcp:` par défaut).
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let parts: Vec<&str> = s.split(':').collect();
        let (proto, ports) = match parts.as_slice() {
            [p, h, g] => (*p, [*h, *g]),
            [h, g] => ("tcp", [*h, *g]),
            _ => return Err(format!("« {s} » : tcp:HÔTE:INVITÉ attendu")),
        };
        let udp = match proto {
            "tcp" => false,
            "udp" => true,
            _ => return Err(format!("« {proto} » : tcp ou udp attendu")),
        };
        let port = |p: &str| p.parse::<u16>().map_err(|_| format!("« {p} » : numéro de port attendu"));
        Ok(Self { udp, host: port(ports[0])?, guest: port(ports[1])? })
    }
}

/// Lien avec le thread réseau.
pub struct NetLink {
    outgoing: Arc<Mutex<VecDeque<Vec<u8>>>>,
    /// Écrire un octet réveille le thread ; la fermeture l'arrête.
    wake: OwnedFd,
    incoming: Receiver<Vec<u8>>,
}

extern "C" fn on_frame(user: *mut libc::c_void, frame: *const u8, len: usize) {
    // SAFETY: `user` est le `Sender` du thread réseau, vivant pendant toute
    // la boucle ; `frame` désigne `len` octets valides pendant l'appel.
    let (tx, data) = unsafe { (&*(user as *const Sender<Vec<u8>>), std::slice::from_raw_parts(frame, len)) };
    let _ = tx.send(data.to_vec());
}

impl NetLink {
    pub fn start(forwards: &[Forward]) -> Result<Self> {
        let mut fds = [0; 2];
        // SAFETY: tableau de deux descripteurs, fourni à pipe2.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: descripteurs neufs, possédés à partir d'ici.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        let outgoing = Arc::new(Mutex::new(VecDeque::new()));
        let (tx, incoming) = mpsc::channel();
        let (ready_tx, ready) = mpsc::channel::<std::result::Result<(), String>>();
        let queue = outgoing.clone();
        let forwards = forwards.to_vec();
        std::thread::Builder::new().name("net".into()).spawn(move || {
            let tx: Box<Sender<Vec<u8>>> = Box::new(tx);
            let user = &*tx as *const Sender<Vec<u8>> as *mut libc::c_void;
            // SAFETY: rappel et contexte valides tant que ce thread vit.
            let net = unsafe { bare86_net_new(on_frame, user) };
            if net.is_null() {
                let _ = ready_tx.send(Err("libslirp : initialisation impossible".into()));
                return;
            }
            for f in &forwards {
                // SAFETY: `net` valide.
                if unsafe { bare86_net_hostfwd(net, f.udp.into(), f.host.into(), f.guest.into()) } != 0 {
                    let _ = ready_tx.send(Err(format!("redirection du port {} impossible", f.host)));
                    // SAFETY: plus aucun usage de `net` après.
                    unsafe { bare86_net_free(net) };
                    return;
                }
            }
            let _ = ready_tx.send(Ok(()));
            // Jusqu'à la fermeture du lien (tube fermé).
            run(net, &read, &queue);
            // SAFETY: plus aucun usage de `net` après.
            unsafe { bare86_net_free(net) };
        })?;
        ready.recv().map_err(|_| "thread réseau arrêté")??;
        Ok(Self { outgoing, wake: write, incoming })
    }

    /// Trame émise par l'invité.
    pub fn send(&self, frame: Vec<u8>) {
        if let Ok(mut q) = self.outgoing.lock() {
            q.push_back(frame);
        }
        // SAFETY: un octet écrit dans le tube ; plein = déjà réveillé.
        unsafe { libc::write(self.wake.as_raw_fd(), [1u8].as_ptr().cast(), 1) };
    }

    /// Trame destinée à l'invité, s'il y en a une.
    pub fn try_recv(&self) -> Option<Vec<u8>> {
        self.incoming.try_recv().ok()
    }
}

fn run(net: *mut RawNet, wake: &OwnedFd, queue: &Mutex<VecDeque<Vec<u8>>>) {
    let mut buf = [0u8; 256];
    loop {
        // SAFETY: `net` valide pendant toute la boucle.
        let woken = unsafe { bare86_net_poll(net, wake.as_raw_fd(), 1000) } != 0;
        if woken {
            // SAFETY: lecture dans un tampon local.
            let n = unsafe { libc::read(wake.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n == 0 {
                return;
            }
        }
        let frames: Vec<Vec<u8>> = match queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => return,
        };
        for f in frames {
            // SAFETY: trame locale, `net` valide.
            unsafe { bare86_net_input(net, f.as_ptr(), f.len()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_syntax() {
        assert_eq!("tcp:2323:23".parse::<Forward>(), Ok(Forward { udp: false, host: 2323, guest: 23 }));
        assert_eq!("8080:80".parse::<Forward>(), Ok(Forward { udp: false, host: 8080, guest: 80 }));
        assert_eq!("udp:5000:5000".parse::<Forward>().map(|f| f.udp), Ok(true));
        assert!("icmp:1:2".parse::<Forward>().is_err());
        assert!("tcp:99999:1".parse::<Forward>().is_err());
    }

    /// Requête DHCP DISCOVER émise vers slirp : il répond par une OFFER
    /// (10.0.2.15). Aucun accès au réseau réel.
    #[test]
    fn slirp_answers_dhcp() {
        let link = NetLink::start(&[]).unwrap();
        link.send(dhcp_discover());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let offer = loop {
            if let Some(f) = link.try_recv() {
                break f;
            }
            assert!(std::time::Instant::now() < deadline, "pas de réponse DHCP");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        // Ethernet (14) + IP (20) + UDP (8) ; yiaddr à l'octet 16 du BOOTP.
        assert_eq!(&offer[12..14], [0x08, 0x00]);
        assert_eq!(&offer[14 + 20 + 8 + 16..][..4], [10, 0, 2, 15]);
    }

    fn dhcp_discover() -> Vec<u8> {
        let mut bootp = vec![0u8; 240];
        bootp[0] = 1; // BOOTREQUEST
        bootp[1] = 1;
        bootp[2] = 6;
        bootp[4..8].copy_from_slice(&[1, 2, 3, 4]);
        bootp[28..34].copy_from_slice(&crate::ne2000::MAC);
        bootp[236..240].copy_from_slice(&[99, 130, 83, 99]);
        bootp.extend_from_slice(&[53, 1, 1, 255]);
        let udp_len = 8 + bootp.len();
        let mut udp = vec![0, 68, 0, 67, (udp_len >> 8) as u8, udp_len as u8, 0, 0];
        udp.extend_from_slice(&bootp);
        let ip_len = 20 + udp.len();
        let mut ip = vec![0x45, 0, (ip_len >> 8) as u8, ip_len as u8, 0, 0, 0, 0, 64, 17, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255];
        let mut sum: u32 = ip.chunks(2).map(|c| u32::from(u16::from_be_bytes([c[0], c[1]]))).sum();
        while sum > 0xFFFF {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        let ck = !(sum as u16);
        ip[10..12].copy_from_slice(&ck.to_be_bytes());
        ip.extend_from_slice(&udp);
        let mut eth = vec![0xFF; 6];
        eth.extend_from_slice(&crate::ne2000::MAC);
        eth.extend_from_slice(&[0x08, 0x00]);
        eth.extend_from_slice(&ip);
        eth
    }
}
