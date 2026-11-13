//! SXC USB/Host Controller (`0x21801000`).

use crate::bus::{Cx, Device};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Shared NAND page address state between the SXC NAND controller and DMA.
#[derive(Default)]
pub struct NandState {
    pub page_addr: u32,
    pub raw_count: usize,
}

pub type NandLink = Arc<Mutex<NandState>>;

#[derive(Clone, Serialize, Deserialize)]
struct State {
    regs: BTreeMap<u64, u32>,
}

/// SXC USB/Host & NAND Flash controller (`0x21801000`).
pub struct UsbController {
    base: u64,
    regs: BTreeMap<u64, u32>,
    nand: NandLink,
}

impl UsbController {
    /// Construct an SXC USB/Host controller at `base`.
    pub fn new(base: u64) -> Self {
        Self::with_nand(base, Arc::default())
    }

    /// Construct an SXC USB/Host & NAND controller sharing `nand` state with DMA.
    pub fn with_nand(base: u64, nand: NandLink) -> Self {
        let mut regs = BTreeMap::new();
        regs.insert(0x6c, 1);
        Self { base, regs, nand }
    }
}

impl Device for UsbController {
    fn name(&self) -> &'static str {
        "sxc-usb-ctrl"
    }

    fn read(&mut self, _cx: &mut Cx<'_>, address: u64, _size: u8) -> u64 {
        let off = address - self.base;
        match off {
            0x08 | 0x0a => 0xf1ec,
            // NAND FIFO spare read: byte 7 must be 0xff for valid ECC/spare marker (0x9b38, 0x9b4c).
            0x1c => 0xff00,
            // bit0 ready, bit 1 ready, bits 2..5 = per-512-byte-sector ECC/transfer done (0x6d7c, 0x6fb8, 0x6ff4, 0x9790, 0x97a0).
            0x20 | 0x22 => 0x003f,
            0x42 => 0x0000,
            _ => u64::from(*self.regs.get(&off).unwrap_or(&0)),
        }
    }

    fn write(&mut self, _cx: &mut Cx<'_>, address: u64, _size: u8, value: u64) {
        let off = address - self.base;
        let mut v = value as u32;
        if off == 0x04 {
            v = 0;
        } else if off == 0x14 {
            if let Ok(mut n) = self.nand.lock() {
                n.page_addr = (n.page_addr & 0xffff_0000) | (v & 0xffff);
            }
        } else if off == 0x16 {
            if let Ok(mut n) = self.nand.lock() {
                n.page_addr = (n.page_addr & 0x0000_ffff) | ((v & 0xffff) << 16);
            }
        }
        self.regs.insert(off, v);
    }

    fn fingerprint(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(State {
            regs: self.regs.clone(),
        })
        .map_err(|e| e.to_string())
    }
}
