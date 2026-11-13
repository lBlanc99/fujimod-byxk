//! SXC Interrupt Controller (`0x22800000`).

use crate::bus::{CoreInterrupts, Cx, Device};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

const SPURIOUS_IRQ: u32 = 0x1f;

#[derive(Default)]
struct Shared {
    pending: Mutex<[u32; 4]>,
    irq: AtomicBool,
}

/// Shared interrupt input lines for [`InterruptController`].
#[derive(Clone, Default)]
pub struct Inputs(Arc<Shared>);

impl Inputs {
    fn refresh(&self) {
        let pending = self.0.pending.lock().unwrap();
        let high = pending.iter().any(|&w| w != 0);
        self.0.irq.store(high, Ordering::Release);
    }

    fn clear_word(&self, word: usize, mask: u32) {
        {
            let mut pending = self.0.pending.lock().unwrap();
            pending[word] &= mask;
        }
        self.refresh();
    }

    fn active_irq(&self) -> u32 {
        let pending = self.0.pending.lock().unwrap();
        for (w, &bits) in pending.iter().enumerate() {
            if bits != 0 {
                return (w as u32) * 32 + bits.trailing_zeros();
            }
        }
        SPURIOUS_IRQ
    }
}

impl CoreInterrupts for Inputs {
    fn pulse(&self, line: u32) -> Result<(), String> {
        self.level(line, true)
    }

    fn level(&self, line: u32, high: bool) -> Result<(), String> {
        if line >= 128 {
            return Err(format!("SXC IRQ line {line} out of range"));
        }
        let word = (line / 32) as usize;
        let bit = 1u32 << (line % 32);
        {
            let mut pending = self.0.pending.lock().unwrap();
            if high {
                pending[word] |= bit;
            } else {
                pending[word] &= !bit;
            }
        }
        self.refresh();
        Ok(())
    }

    fn irq(&self) -> bool {
        self.0.irq.load(Ordering::Acquire)
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct State {
    pending: [u32; 4],
    regs: BTreeMap<u64, u32>,
}

/// SXC 128-line Interrupt Controller.
pub struct InterruptController {
    base: u64,
    inputs: Inputs,
    regs: BTreeMap<u64, u32>,
}

impl InterruptController {
    /// Construct an SXC interrupt controller at `base` using `inputs`.
    pub fn new(base: u64, inputs: Inputs) -> Self {
        Self {
            base,
            inputs,
            regs: BTreeMap::new(),
        }
    }
}

impl Device for InterruptController {
    fn name(&self) -> &'static str {
        "sxc-intc"
    }

    fn read(&mut self, _cx: &mut Cx<'_>, address: u64, _size: u8) -> u64 {
        let off = address - self.base;
        if off == 0 {
            return u64::from(self.inputs.active_irq());
        }
        u64::from(*self.regs.get(&off).unwrap_or(&0))
    }

    fn write(&mut self, _cx: &mut Cx<'_>, address: u64, _size: u8, value: u64) {
        let off = address - self.base;
        let v = value as u32;
        if (0x04..=0x10).contains(&off) && (off - 0x04) % 4 == 0 {
            let word = ((off - 0x04) / 4) as usize;
            if word < 4 {
                self.inputs.clear_word(word, v);
            }
        }
        self.regs.insert(off, v);
    }

    fn fingerprint(&self) -> Result<serde_json::Value, String> {
        let pending = *self.inputs.0.pending.lock().unwrap();
        serde_json::to_value(State {
            pending,
            regs: self.regs.clone(),
        })
        .map_err(|e| e.to_string())
    }
}
