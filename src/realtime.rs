//! Priorité temps réel du thread du vCPU (`--realtime`).
//!
//! Sous l'ordonnanceur ordinaire, une autre tâche de l'hôte peut préempter
//! le thread du vCPU pendant plusieurs millisecondes : le signal d'une
//! échéance de minuterie arrive alors en retard, et une IRQ 0 levée trop
//! tard fait rater leur synchronisation au balayage aux démos qui en
//! dépendent (ville de Second Reality : ≈ 1,25 % de glissement, journal
//! point 8). En SCHED_RR, le thread passe devant toutes les tâches
//! ordinaires. Le noyau en borne la part (`sched_rt_runtime_us`, 95 % par
//! défaut), et le thread dort dès que l'invité attend.

/// Priorité modeste : au-dessus de toute tâche ordinaire, sous les threads
/// temps réel du système (audio de PipeWire, IRQ en threads).
const PRIORITY: i32 = 10;

/// Passe le thread appelant en SCHED_RR ; les processus qu'il lancerait
/// reviennent à l'ordonnanceur ordinaire (SCHED_RESET_ON_FORK).
pub fn enable() {
    let param = libc::sched_param { sched_priority: PRIORITY };
    // SAFETY: appel système sur le thread appelant (pid 0), paramètre valide.
    let r = unsafe { libc::sched_setscheduler(0, libc::SCHED_RR | libc::SCHED_RESET_ON_FORK, &param) };
    if r != 0 {
        let e = std::io::Error::last_os_error();
        let limit = rtprio_limit();
        eprintln!("--realtime: {e} (RLIMIT_RTPRIO = {limit}, {PRIORITY} needed); running at normal priority");
    }
}

fn rtprio_limit() -> u64 {
    let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: lecture d'une limite dans une structure locale.
    unsafe { libc::getrlimit(libc::RLIMIT_RTPRIO, &mut r) };
    r.rlim_cur
}
