// SPDX-License-Identifier: GPL-3.0-or-later
//! Réseau en espace utilisateur (« user networking » de QEMU) : libslirp
//! tourne dans son propre thread et fait le NAT des trames de l'invité vers
//! les sockets de l'hôte. Réseau 10.0.2.0/24 : invité 10.0.2.15 (DHCP),
//! passerelle 10.0.2.2, DNS 10.0.2.3. Aucun droit particulier requis.

use std::collections::VecDeque;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
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
    kick: Arc<Kick>,
}

/// Réveil du vCPU à l'arrivée de trames : un eventfd pour l'attente en HLT,
/// le signal du battement pour sortir de `KVM_RUN`. Un seul réveil tant que
/// le vCPU n'a pas relevé les trames.
struct Kick {
    event: OwnedFd,
    /// Thread du vCPU.
    tid: libc::pid_t,
    pending: AtomicBool,
}

impl Kick {
    fn fire(&self) {
        if self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let one = 1u64;
        // SAFETY: écriture de 8 octets dans un eventfd ; signal sans effet
        // autre que l'interruption d'un appel système (gestionnaire vide).
        unsafe {
            libc::write(self.event.as_raw_fd(), (&raw const one).cast(), 8);
            libc::syscall(libc::SYS_tgkill, libc::getpid(), self.tid, crate::timer::signal());
        }
    }
}

/// Contexte des rappels de slirp, dans le thread réseau.
struct RxContext {
    tx: Sender<Vec<u8>>,
    /// Trames reçues pendant le tour en cours.
    got: bool,
}

extern "C" fn on_frame(user: *mut libc::c_void, frame: *const u8, len: usize) {
    // SAFETY: `user` est le `RxContext` du thread réseau, vivant pendant
    // toute la boucle et seulement utilisé par elle ; `frame` désigne `len`
    // octets valides pendant l'appel.
    let (ctx, data) = unsafe { (&mut *(user as *mut RxContext), std::slice::from_raw_parts(frame, len)) };
    let _ = ctx.tx.send(data.to_vec());
    ctx.got = true;
    log_frame("->G", data);
}

impl NetLink {
    /// À appeler depuis le thread du vCPU (cible des réveils).
    pub fn start(forwards: &[Forward]) -> Result<Self> {
        crate::timer::install_handler()?;
        // SAFETY: eventfd neuf, possédé à partir d'ici.
        let event = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if event < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: descripteur valide, possédé ; gettid sans condition.
        let kick = Arc::new(Kick { event: unsafe { OwnedFd::from_raw_fd(event) }, tid: unsafe { libc::gettid() }, pending: AtomicBool::new(false) });
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
        let kicker = kick.clone();
        std::thread::Builder::new().name("net".into()).spawn(move || {
            let mut ctx = Box::new(RxContext { tx, got: false });
            let user = &mut *ctx as *mut RxContext as *mut libc::c_void;
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
            run(net, &read, &queue, user.cast(), &kicker);
            // SAFETY: plus aucun usage de `net` après.
            unsafe { bare86_net_free(net) };
        })?;
        ready.recv().map_err(|_| "thread réseau arrêté")??;
        Ok(Self { outgoing, wake: write, incoming, kick })
    }

    /// Trame émise par l'invité.
    pub fn send(&self, frame: Vec<u8>) {
        if let Ok(mut q) = self.outgoing.lock() {
            q.push_back(frame);
        }
        // SAFETY: un octet écrit dans le tube ; plein = déjà réveillé.
        unsafe { libc::write(self.wake.as_raw_fd(), [1u8].as_ptr().cast(), 1) };
    }

    /// Trames destinées à l'invité. Réarme le réveil d'abord : une trame
    /// arrivée pendant la relève en redéclenche un.
    pub fn receive(&self, mut deliver: impl FnMut(Vec<u8>)) {
        self.kick.pending.store(false, Ordering::Release);
        while let Ok(f) = self.incoming.try_recv() {
            deliver(f);
        }
    }

    /// Descripteur lisible quand des trames attendent (attente en HLT).
    pub fn event_fd(&self) -> libc::c_int {
        self.kick.event.as_raw_fd()
    }

    /// Vide l'eventfd après une attente.
    pub fn clear_event(&self) {
        let mut v = 0u64;
        // SAFETY: lecture de 8 octets d'un eventfd non bloquant.
        unsafe { libc::read(self.kick.event.as_raw_fd(), (&raw mut v).cast(), 8) };
    }
}

fn run(net: *mut RawNet, wake: &OwnedFd, queue: &Mutex<VecDeque<Vec<u8>>>, ctx: *mut RxContext, kick: &Kick) {
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
            log_frame("G->", &f);
            // SAFETY: trame locale, `net` valide.
            unsafe { bare86_net_input(net, f.as_ptr(), f.len()) };
        }
        // SAFETY: contexte des rappels, utilisé seulement par ce thread et
        // hors de tout rappel ici.
        let ctx = unsafe { &mut *ctx };
        if std::mem::take(&mut ctx.got) {
            kick.fire();
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
            let mut got = None;
            link.receive(|f| got = got.take().or(Some(f)));
            if let Some(f) = got {
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

/// Trace des segments TCP (`BARE86_NETLOG=1`) : instant en µs, sens (G =
/// invité), numéros de séquence et d'acquittement, fenêtre, charge utile.
/// Sert à mesurer le débit (`tools/netbench.sh`).
fn log_frame(dir: &str, f: &[u8]) {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    // IPv4 (0800h), TCP (protocole 6), en-tête IP de 20 octets.
    if !*ON.get_or_init(|| std::env::var_os("BARE86_NETLOG").is_some()) || f.len() < 54 || f[12..14] != [8, 0] || f[23] != 6 || f[14] != 0x45 {
        return;
    }
    let t = T0.get_or_init(std::time::Instant::now).elapsed().as_micros();
    let tcp = &f[34..];
    let seq = u32::from_be_bytes([tcp[4], tcp[5], tcp[6], tcp[7]]);
    let ack = u32::from_be_bytes([tcp[8], tcp[9], tcp[10], tcp[11]]);
    let win = u16::from_be_bytes([tcp[14], tcp[15]]);
    let iplen = usize::from(u16::from_be_bytes([f[16], f[17]]));
    let payload = iplen.saturating_sub(20 + usize::from(tcp[12] >> 4) * 4);
    eprintln!("{t:>9} {dir} seq={seq} ack={ack} win={win} len={payload}");
}
