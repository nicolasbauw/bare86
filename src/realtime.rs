//! Priorité temps réel du thread du vCPU (d'office, `--no-realtime` la coupe).
//!
//! Sous l'ordonnanceur ordinaire, une autre tâche de l'hôte peut préempter
//! le thread du vCPU pendant plusieurs millisecondes : le signal d'une
//! échéance de minuterie arrive alors en retard, et une IRQ 0 levée trop
//! tard fait rater leur synchronisation au balayage aux démos qui en
//! dépendent (ville de Second Reality : ≈ 1,25 % de glissement, journal
//! point 8). En SCHED_RR, le thread passe devant toutes les tâches
//! ordinaires, et il dort dès que l'invité attend.
//!
//! Deux voies, essayées dans l'ordre, en silence si aucune n'aboutit :
//! 1. directe, quand la limite `RLIMIT_RTPRIO` de l'utilisateur le permet
//!    (groupe `realtime` d'Arch, `limits.d` ailleurs) ;
//! 2. par rtkit (`org.freedesktop.RealtimeKit1` sur le bus système), le
//!    service qui accorde cette priorité à PipeWire et PulseAudio sans
//!    réglage, présent d'office sur Ubuntu ou Fedora. Il l'accorde jusqu'à
//!    la priorité 20, à un processus qui borne son temps de calcul temps
//!    réel sans dormir (`RLIMIT_RTTIME`, au plus 200 ms par défaut) : au-delà
//!    de la limite douce, SIGXCPU, de la dure, la mort du processus. Or un
//!    invité calcule souvent bien plus longtemps sans attendre (démarrage,
//!    chargements, timedemo). Le thread dort donc 10 µs toutes les 40 ms
//!    (`breathe`, ≈ 0,03 % du temps), ce qui remet le compte à zéro, comme
//!    il le serait sur la voie directe faute de limite ; si cela ne suffit
//!    pas (long appel à l'hôte), le gestionnaire de SIGXCPU rend au thread
//!    la priorité normale avant la limite dure.

use std::process::{Command, Stdio};
use std::cell::Cell;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

/// Priorité modeste : au-dessus de toute tâche ordinaire, sous les threads
/// temps réel du système (audio de PipeWire, IRQ en threads) et sous le
/// plafond de rtkit (20).
const PRIORITY: i32 = 10;

/// Limites du temps de calcul temps réel sans dormir, en µs (voie rtkit) :
/// douce (SIGXCPU, retour à la priorité normale), dure (`RTTimeUSecMax` de
/// rtkit par défaut).
const RTTIME_SOFT_US: u64 = 100_000;
const RTTIME_HARD_US: u64 = 200_000;

/// Thread passé en temps réel par rtkit (0 : aucun), pour le gestionnaire
/// de SIGXCPU, qui peut s'exécuter sur un autre thread.
static RT_TID: AtomicI32 = AtomicI32::new(0);

/// Intervalle entre deux sommeils de `breathe`, bien sous la limite douce.
const BREATH_EVERY: Duration = Duration::from_millis(40);
/// Sommeil assez long pour que le thread quitte vraiment le cœur (une
/// échéance déjà passée rendrait la main sans dormir).
const BREATH: Duration = Duration::from_micros(10);

thread_local! {
    static LAST_BREATH: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// À appeler souvent sur le thread du vCPU (haut de la boucle) : sur la voie
/// rtkit, dort brièvement toutes les `BREATH_EVERY` pour remettre à zéro le
/// compte de `RLIMIT_RTTIME`. Sans effet sur les autres voies.
pub fn breathe(now: Instant) {
    if RT_TID.load(Ordering::Relaxed) == 0 {
        return;
    }
    LAST_BREATH.with(|last| match last.get() {
        Some(t) if now.duration_since(t) < BREATH_EVERY => {}
        _ => {
            std::thread::sleep(BREATH);
            last.set(Some(now));
        }
    });
}

/// Passe le thread appelant en SCHED_RR si le droit le permet, directement
/// ou par rtkit ; les processus qu'il lancerait reviennent à l'ordonnanceur
/// ordinaire (SCHED_RESET_ON_FORK).
pub fn enable() {
    if !direct() {
        rtkit();
    }
}

fn direct() -> bool {
    let param = libc::sched_param { sched_priority: PRIORITY };
    // SAFETY: appel système sur le thread appelant (pid 0), paramètre valide.
    // Sans le droit (`RLIMIT_RTPRIO` trop bas), l'appel échoue.
    unsafe { libc::sched_setscheduler(0, libc::SCHED_RR | libc::SCHED_RESET_ON_FORK, &param) == 0 }
}

fn rtkit() {
    // SAFETY: appels système sans pointeur, ou sur des structures locales.
    let (pid, tid) = unsafe { (libc::getpid(), libc::gettid()) };
    let limit = libc::rlimit { rlim_cur: RTTIME_SOFT_US, rlim_max: RTTIME_HARD_US };
    if unsafe { libc::setrlimit(libc::RLIMIT_RTTIME, &limit) } != 0 {
        return;
    }
    RT_TID.store(tid, Ordering::Relaxed);
    // SAFETY: gestionnaire réduit à un appel système (sûr dans un signal).
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_sigxcpu as *const () as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = libc::SA_RESTART;
        libc::sigaction(libc::SIGXCPU, &sa, std::ptr::null_mut());
    }
    // Refus ou rtkit absent : la limite reste, sans effet (elle ne compte
    // que le temps passé en temps réel).
    if !ask_rtkit(pid as u64, tid as u64) {
        RT_TID.store(0, Ordering::Relaxed);
    }
}

/// Demande à rtkit, par l'un des deux clients D-Bus courants (busctl de
/// systemd, dbus-send ailleurs), de passer `tid` en temps réel.
fn ask_rtkit(pid: u64, tid: u64) -> bool {
    const DEST: &str = "org.freedesktop.RealtimeKit1";
    const PATH: &str = "/org/freedesktop/RealtimeKit1";
    let prio = PRIORITY.to_string();
    let mut busctl = Command::new("busctl");
    busctl.args(["--system", "--timeout=2", "call", DEST, PATH, DEST, "MakeThreadRealtimeWithPID", "ttu"]);
    busctl.args([pid.to_string(), tid.to_string(), prio.clone()]);
    if run(&mut busctl) {
        return true;
    }
    let method = format!("{DEST}.MakeThreadRealtimeWithPID");
    let mut dbus_send = Command::new("dbus-send");
    dbus_send.args(["--system", "--print-reply", "--reply-timeout=2000", &format!("--dest={DEST}"), PATH, &method]);
    dbus_send.args([format!("uint64:{pid}"), format!("uint64:{tid}"), format!("uint32:{prio}")]);
    run(&mut dbus_send)
}

fn run(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.status().is_ok_and(|s| s.success())
}

/// Limite douce de `RLIMIT_RTTIME` atteinte : l'invité calcule sans jamais
/// laisser dormir le thread ; retour à la priorité normale avant que la
/// limite dure ne tue le processus.
extern "C" fn on_sigxcpu(_: libc::c_int) {
    let tid = RT_TID.swap(0, Ordering::Relaxed);
    if tid != 0 {
        let param = libc::sched_param { sched_priority: 0 };
        // SCHED_RESET_ON_FORK, posé par rtkit, gardé : un processus non
        // privilégié ne peut pas le retirer (EPERM).
        // SAFETY: appel système sûr dans un gestionnaire de signal.
        unsafe { libc::sched_setscheduler(tid, libc::SCHED_OTHER | libc::SCHED_RESET_ON_FORK, &param) };
    }
}
