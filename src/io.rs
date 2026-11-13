// SPDX-License-Identifier: GPL-3.0-or-later
//! Bus des ports d'E/S : chaque périphérique émulé déclare ses ports.

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

pub trait IoDevice {
    /// `size` : 1, 2 ou 4 octets.
    fn read(&mut self, port: u16, size: u8) -> u32;
    fn write(&mut self, port: u16, size: u8, value: u32);
}

#[derive(Default)]
pub struct IoBus {
    devices: Vec<(RangeInclusive<u16>, Box<dyn IoDevice>)>,
    /// Ports sans périphérique déjà signalés.
    unmapped: BTreeSet<u16>,
    pub trace: bool,
}

impl IoBus {
    pub fn map(&mut self, ports: RangeInclusive<u16>, dev: Box<dyn IoDevice>) {
        self.devices.push((ports, dev));
    }

    fn find(&mut self, port: u16) -> Option<&mut Box<dyn IoDevice>> {
        self.devices.iter_mut().find(|(r, _)| r.contains(&port)).map(|(_, d)| d)
    }

    fn note_unmapped(&mut self, port: u16, what: &str) {
        if self.trace && self.unmapped.insert(port) {
            eprintln!("[io] {what} port {port:04X}h sans périphérique");
        }
    }

    pub fn read(&mut self, port: u16, data: &mut [u8]) {
        let size = data.len() as u8;
        let v = match self.find(port) {
            Some(d) => d.read(port, size),
            None => {
                self.note_unmapped(port, "IN");
                u32::MAX
            }
        };
        data.copy_from_slice(&v.to_le_bytes()[..data.len()]);
    }

    pub fn write(&mut self, port: u16, data: &[u8]) {
        let mut b = [0u8; 4];
        b[..data.len()].copy_from_slice(data);
        let v = u32::from_le_bytes(b);
        match self.find(port) {
            Some(d) => d.write(port, data.len() as u8, v),
            None => self.note_unmapped(port, "OUT"),
        }
    }
}

/// Port sans effet : écritures ignorées, lectures à FFh.
pub struct Sink;

impl IoDevice for Sink {
    fn read(&mut self, _: u16, _: u8) -> u32 {
        u32::MAX
    }
    fn write(&mut self, _: u16, _: u8, _: u32) {}
}

/// Périphérique partagé entre plusieurs plages de ports.
pub struct Shared<T: IoDevice>(pub std::rc::Rc<std::cell::RefCell<T>>);

impl<T: IoDevice> IoDevice for Shared<T> {
    fn read(&mut self, port: u16, size: u8) -> u32 {
        self.0.borrow_mut().read(port, size)
    }
    fn write(&mut self, port: u16, size: u8, value: u32) {
        self.0.borrow_mut().write(port, size, value)
    }
}
