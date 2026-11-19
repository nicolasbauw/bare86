// SPDX-License-Identifier: GPL-3.0-or-later
//! Enregistrement vidéo de la session (`--video F.mkv`), pour comparer avec
//! une référence (`tools/compare.py`) : une image toutes les 1/70 s de temps
//! réel, la dernière répétée si la machine n'en a pas rendu de nouvelle,
//! toujours en 640×480 (mise à l'échelle au plus proche), encodée par
//! `ffmpeg` (x264, sans perte visible). La cadence fixe permet de compter les
//! images distinctes par seconde, c'est-à-dire la fluidité réelle.
//!
//! Mise à l'échelle et écriture vers ffmpeg (≈ 86 Mo/s) dans un thread à
//! part : faites dans le thread du vCPU, elles retardaient l'IRQ du
//! minuteur au point de faire manquer leurs retours de trame aux démos
//! (Second Reality à 45 % de sa vitesse : la mesure faussait le mesuré).

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 480;
pub const FPS: u32 = 70;

/// Image de la machine (largeur, hauteur, pixels 0x00RRGGBB), ou `None` si
/// la précédente reste ; nombre de fois qu'elle doit être écrite.
type Job = (Option<(usize, usize, Vec<u32>)>, u64);

pub struct VideoOut {
    tx: Option<SyncSender<Job>>,
    writer: Option<JoinHandle<()>>,
    start: Instant,
    frames: u64,
}

impl VideoOut {
    pub fn start(path: &str) -> Result<Self, String> {
        let child = Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "bgr0", "-s"])
            .arg(format!("{WIDTH}x{HEIGHT}"))
            .args(["-r", &FPS.to_string(), "-i", "-", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "16", "-pix_fmt", "yuv420p"])
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("--video: ffmpeg: {e}"))?;
        // File courte : si l'encodage prend du retard, le vCPU attend plutôt
        // que d'accumuler des images.
        let (tx, rx) = sync_channel(8);
        let writer = std::thread::Builder::new().name("video".into()).spawn(move || write_frames(child, rx)).map_err(|e| format!("--video: {e}"))?;
        Ok(Self { tx: Some(tx), writer: Some(writer), start: Instant::now(), frames: 0 })
    }

    fn time_of(&self, n: u64) -> Instant {
        self.start + Duration::from_nanos(n * 1_000_000_000 / u64::from(FPS))
    }

    /// Une image est due.
    pub fn due(&self, now: Instant) -> bool {
        now >= self.time_of(self.frames)
    }

    /// Image courante de la machine (`None` : la précédente reste), à écrire
    /// autant de fois qu'il faut pour rattraper l'horloge.
    pub fn push(&mut self, frame: Option<(usize, usize, &[u32])>, now: Instant) {
        let mut n = 0;
        while self.due(now) {
            self.frames += 1;
            n += 1;
        }
        if n == 0 {
            return;
        }
        let job = (frame.map(|(w, h, px)| (w, h, px.to_vec())), n);
        if let Some(tx) = &self.tx
            && tx.send(job).is_err()
        {
            eprintln!("--video: encoder stopped");
            self.tx = None;
        }
    }
}

fn write_frames(mut child: Child, rx: Receiver<Job>) {
    let mut stdin = child.stdin.take();
    let mut image = vec![0u8; WIDTH * HEIGHT * 4];
    for (frame, n) in rx {
        if let Some((w, h, px)) = frame
            && w > 0
            && h > 0
        {
            for y in 0..HEIGHT {
                let row = &px[(y * h / HEIGHT) * w..];
                for x in 0..WIDTH {
                    image[(y * WIDTH + x) * 4..][..4].copy_from_slice(&row[x * w / WIDTH].to_le_bytes());
                }
            }
        }
        for _ in 0..n {
            if let Some(s) = &mut stdin
                && s.write_all(&image).is_err()
            {
                eprintln!("--video: ffmpeg stopped");
                stdin = None;
            }
        }
    }
    drop(stdin);
    let _ = child.wait();
}

impl Drop for VideoOut {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}
