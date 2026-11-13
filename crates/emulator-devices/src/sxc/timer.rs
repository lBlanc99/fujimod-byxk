//! SXC 8-channel Timer (`0x22810000`).

use crate::bus::{Cx, Device};
use serde::{Deserialize, Serialize};

const CHANNELS: usize = 8;
const TIMER_IRQ_LINE: u32 = 4;
const DEFAULT_TICK_STEP: u64 = 2000;

#[derive(Clone, Serialize, Deserialize)]
struct State {
    ctrl: [u32; CHANNELS],
    enable: [u32; CHANNELS],
    global_enable: u32,
    pending: u32,
    next_deadline: Option<u64>,
}

/// SXC 8-channel timer firing IRQ 4 on CPU0.
pub struct Timers {
    base: u64,
    ctrl: [u32; CHANNELS],
    enable: [u32; CHANNELS],
    global_enable: u32,
    pending: u32,
    next_deadline: Option<u64>,
}

impl Timers {
    /// Construct an SXC timer block at `base`.
    pub fn new(base: u64) -> Self {
        Self {
            base,
            ctrl: [0x40; CHANNELS],
            enable: [0; CHANNELS],
            global_enable: 0,
            pending: 0,
            next_deadline: None,
        }
    }

    fn schedule_next(&mut self, cx: &mut Cx<'_>) {
        if self.global_enable != 0 && self.enable.iter().any(|&e| e != 0) {
            let at = cx.now().saturating_add(DEFAULT_TICK_STEP);
            self.next_deadline = Some(at);
            cx.schedule(at, 1);
        } else {
            self.next_deadline = None;
        }
    }
}

impl Device for Timers {
    fn name(&self) -> &'static str {
        "sxc-timers"
    }

    fn read(&mut self, cx: &mut Cx<'_>, address: u64, _size: u8) -> u64 {
        let off = address - self.base;
        if off < 0x80 {
            let ch = (off / 0x10) as usize;
            return match off % 0x10 {
                0x0 => u64::from(self.ctrl[ch]),
                0x4 => cx.now() & 0xffff_ffff,
                0x8 => u64::from(self.enable[ch]),
                _ => 0,
            };
        }
        match off {
            0x80 => u64::from(self.global_enable),
            0x84 => u64::from(self.pending),
            _ => 0,
        }
    }

    fn write(&mut self, cx: &mut Cx<'_>, address: u64, _size: u8, value: u64) {
        let off = address - self.base;
        let v = value as u32;
        if off < 0x80 {
            let ch = (off / 0x10) as usize;
            match off % 0x10 {
                0x0 => self.ctrl[ch] = v,
                0x8 => {
                    self.enable[ch] = v & 1;
                    self.schedule_next(cx);
                }
                _ => {}
            }
            return;
        }
        match off {
            0x80 => {
                self.global_enable = v & 1;
                self.schedule_next(cx);
            }
            0x84 => {
                self.pending &= v;
            }
            _ => {}
        }
    }

    fn event(&mut self, cx: &mut Cx<'_>, _token: u64) {
        if self.global_enable != 0 {
            let mut fired = false;
            for ch in 0..CHANNELS {
                if self.enable[ch] != 0 && (self.ctrl[ch] & 0x40) == 0 {
                    self.pending |= 1 << ch;
                    fired = true;
                }
            }
            if fired {
                let _ = cx.raise_irq(0, TIMER_IRQ_LINE);
            }
        }
        self.schedule_next(cx);
    }

    fn next_deadline(&self, _now: u64) -> Option<u64> {
        self.next_deadline
    }

    fn fingerprint(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(State {
            ctrl: self.ctrl,
            enable: self.enable,
            global_enable: self.global_enable,
            pending: self.pending,
            next_deadline: self.next_deadline,
        })
        .map_err(|e| e.to_string())
    }
}
