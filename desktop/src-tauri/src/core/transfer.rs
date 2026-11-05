use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug)]
struct Rec {
    at: Instant,
    bytes: u64,
}

impl Default for Rec {
    fn default() -> Self {
        Self { at: Instant::now(), bytes: 0 }
    }
}

/// État partagé d'une session de téléchargement : drapeau de pause + mesure de vitesse.
/// Le fichier `.part` est écrit de façon incrémentale, donc on peut mettre en pause / reprise sans perte.
#[derive(Debug, Default)]
pub struct TransferState {
    pub paused: AtomicBool,
    speed: AtomicU64,
    rec: Mutex<Rec>,
}

impl TransferState {
    pub fn new() -> Self {
        Self {
            rec: Mutex::new(Rec {
                at: Instant::now(),
                bytes: 0,
            }),
            ..Default::default()
        }
    }

    #[inline]
    pub fn set_paused(&self, v: bool) {
        self.paused.store(v, Ordering::SeqCst);
    }

    #[inline]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Appelé à chaque tick de progression (octets globaux cumulés). Met à jour la vitesse lissée.
    pub fn record(&self, bytes_global: u64) {
        let now = Instant::now();
        let inst = {
            let mut r = self.rec.lock().expect("rec lock poisoned");
            let v = if bytes_global >= r.bytes {
                let dt = now.duration_since(r.at).as_secs_f64();
                if dt > 0.0 {
                    (bytes_global - r.bytes) as f64 / dt
                } else {
                    0.0
                }
            } else {
                0.0
            };
            r.bytes = bytes_global;
            r.at = now;
            v
        };
        let prev = self.speed.load(Ordering::Relaxed) as f64;
        let s = if prev <= 0.0 { inst } else { prev * 0.6 + inst * 0.4 };
        self.speed.store(s as u64, Ordering::Relaxed);
    }

    #[inline]
    pub fn speed_bps(&self) -> u64 {
        if self.is_paused() {
            0
        } else {
            self.speed.load(Ordering::Relaxed)
        }
    }
}
