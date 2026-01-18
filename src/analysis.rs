use crate::ducking::RestoreGuard;
use crate::ControlMode;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

/// atomic f32 bits
#[derive(Debug)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub const fn new(val: f32) -> Self {
        Self(AtomicU32::new(val.to_bits()))
    }
    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    pub fn store(&self, val: f32) {
        self.0.store(val.to_bits(), Ordering::Relaxed);
    }
}

// VAD state
#[derive(Debug)]
pub struct VadState {
    pub last_above: Option<Instant>,
    pub above_start: Option<Instant>,
    pub voice_active: bool,
}

impl VadState {
    pub const fn new(active: bool) -> Self {
        Self {
            last_above: None,
            above_start: None,
            voice_active: active,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct VadSnapshot {
    pub voice_active: bool,
    #[cfg(feature = "dev-tools")]
    pub desired_duck: bool,
    pub applied_duck: bool,
}

#[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
pub fn auto_vad_step(
    mode: ControlMode,
    energy: f32,
    thr: f32,
    now: Instant,
    state: &mut VadState,
    guard: &mut RestoreGuard,
    duck_factor: f32,
    log: &mut dyn FnMut(String),
    attack_ms: u64,
    hold_ms: u64,
) -> VadSnapshot {
    if mode != ControlMode::AutoVad {
        #[cfg(feature = "dev-tools")]
        let desired_duck = matches!(mode, ControlMode::ManualDucked);
        #[cfg(not(feature = "dev-tools"))]
        let _desired_duck = matches!(mode, ControlMode::ManualDucked);
        #[cfg(feature = "dev-tools")]
        return VadSnapshot {
            voice_active: state.voice_active,
            desired_duck,
            applied_duck: guard.ducked,
        };
        #[cfg(not(feature = "dev-tools"))]
        return VadSnapshot {
            voice_active: state.voice_active,
            applied_duck: guard.ducked,
        };
    }

    if energy > thr {
        state.last_above = Some(now);
        if !state.voice_active {
            match state.above_start {
                None => {
                    state.above_start = Some(now);
                }
                Some(start) => {
                    if attack_ms == 0 || now.duration_since(start).as_millis() as u64 >= attack_ms {
                        state.voice_active = true;
                        state.above_start = None;
                    }
                }
            }
        }
    } else {
        state.above_start = None;
        if state.voice_active {
            if let Some(last) = state.last_above {
                if now.duration_since(last).as_millis() as u64 >= hold_ms {
                    state.voice_active = false;
                    state.last_above = None;
                }
            }
        }
    }

    let desired_duck = state.voice_active;
    if desired_duck != guard.ducked {
        if desired_duck {
            log(format!(
                "VOICE ACTIVE (level={energy:.4}) → Ducking einschalten"
            ));
            guard.apply_duck(duck_factor);
        } else {
            log(format!(
                "VOICE INACTIVE (level={energy:.4}) → Ducking ausschalten"
            ));
            guard.restore();
        }
    }

    #[cfg(feature = "dev-tools")]
    {
        VadSnapshot {
            voice_active: state.voice_active,
            desired_duck,
            applied_duck: guard.ducked,
        }
    }
    #[cfg(not(feature = "dev-tools"))]
    {
        VadSnapshot {
            voice_active: state.voice_active,
            applied_duck: guard.ducked,
        }
    }
}
