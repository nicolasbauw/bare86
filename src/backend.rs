// SPDX-License-Identifier: GPL-3.0-or-later
//! Abstraction du moteur de virtualisation : KVM aujourd'hui, WHPX
//! (Windows) ou Hypervisor.framework (macOS Intel) plus tard.

use std::fmt;

use crate::interp::Cpu;
use crate::mem::GuestMem;

/// Raison du retour de l'invité vers l'hôte.
pub enum Exit<'a> {
    /// `size` : taille d'un accès (1, 2 ou 4) ; `data` en contient
    /// plusieurs pour une instruction de chaîne (`rep insw`, `rep outsb`…).
    IoOut { port: u16, size: u8, data: &'a [u8] },
    IoIn { port: u16, size: u8, data: &'a mut [u8] },
    MmioRead { addr: u64, data: &'a mut [u8] },
    MmioWrite { addr: u64, data: &'a [u8] },
    Hlt,
    /// L'invité peut recevoir une interruption (fenêtre demandée).
    IrqWindow,
    /// Exécution interrompue par un signal (bridage, minuterie).
    Interrupted,
    /// Fin d'un pas en mode pas à pas.
    Step,
    /// Point d'arrêt matériel atteint (`BARE86_HWBP`, diagnostic) ;
    /// l'exécution reprend à l'instruction arrêtée.
    Breakpoint,
    /// Triple faute ou arrêt du processeur.
    Shutdown,
    Other(String),
}

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<kvm_ioctls::Error> for Error {
    fn from(e: kvm_ioctls::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Self(e)
    }
}

impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Self(e.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Une VM à un vCPU.
pub trait Backend {
    fn mem(&self) -> &GuestMem;
    /// Exécute l'invité jusqu'à la prochaine sortie.
    fn run(&mut self) -> Result<Exit<'_>>;
    /// Termine l'instruction en cours (accès E/S ou MMIO déjà servi) sans
    /// relancer l'invité. Une instruction à deux accès MMIO (MOVS de la VRAM
    /// vers la VRAM) en produit un second, servi par `mmio(adresse,
    /// données, écriture)`.
    fn complete_pending(&mut self, mmio: &mut dyn FnMut(u64, &mut [u8], bool)) -> Result<()>;
    fn state(&self) -> Result<Cpu>;
    fn set_state(&mut self, cpu: &Cpu) -> Result<()>;
    /// Registres système de la mise sous tension (mode réel, CR0, CR3,
    /// GDT, IDT), pour une réinitialisation de la machine.
    fn reset_cpu(&mut self) -> Result<()>;
    /// Pas à pas : chaque `run` s'arrête après une instruction (`Exit::Step`).
    fn set_single_step(&mut self, on: bool) -> Result<()>;
    /// L'invité accepte une interruption externe maintenant (IF = 1, pas
    /// d'ombre d'interruption) ; d'après la dernière sortie.
    fn can_inject(&mut self) -> bool;
    /// Une interruption injectée attend encore d'être livrée à l'invité
    /// (`KVM_RUN` interrompu avant l'entrée, ou livraison elle-même
    /// interrompue par une sortie) : il faut rentrer dans l'invité.
    fn holds_injection(&mut self) -> bool {
        false
    }
    /// Injecte l'interruption externe `vector` avant la prochaine instruction.
    fn inject(&mut self, vector: u8) -> Result<()>;
    /// Demande une sortie dès que l'invité pourra recevoir une interruption.
    fn request_irq_window(&mut self, on: bool);
    /// Mappe la fenêtre VGA A0000–AFFFF sur 64 Ko de mémoire hôte (accès
    /// natifs), ou la démappe (`None` : accès MMIO). L'adresse est toujours
    /// la même : la vue hôte est remappée par la carte VGA elle-même.
    /// Fenêtre VGA mappée sur `host_addr` (en lecture seule : les écritures
    /// sortent en MMIO), ou démappée.
    fn map_vga(&mut self, host_addr: Option<u64>, readonly: bool) -> Result<()>;
    /// Porte A20 : fermée, la HMA (100000h–10FFEFh) se replie sur 0.
    fn set_a20(&mut self, open: bool) -> Result<()>;
    /// Framebuffer linéaire SVGA : `size` octets de mémoire hôte à
    /// `host_addr`, mappés une fois pour toutes à l'adresse du LFB.
    fn map_lfb(&mut self, host_addr: u64, size: usize) -> Result<()>;

    /// Surveillance des pages de code du traducteur dynamique : les
    /// écritures de l'invité sur `pages` seront notées (journal des pages
    /// modifiées). Faux si ce n'est pas possible.
    fn watch_pages(&mut self, _pages: &[u32]) -> Result<bool> {
        Ok(false)
    }

    /// Pages de `pages` écrites par l'invité depuis leur surveillance,
    /// ajoutées à `out` (et de nouveau surveillées). Faux si la surveillance
    /// n'est pas en place : toute page doit alors être tenue pour écrite.
    fn watched_written(&mut self, _pages: &[u32], _out: &mut Vec<u32>) -> Result<bool> {
        Ok(false)
    }
}
