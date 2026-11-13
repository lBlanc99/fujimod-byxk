//! SXC DMA Channel (`0x300a8000`, `0x300a9000`, `0x300aa000`, `0x300ab000`, `0x300ae000`).

use super::usb_ctrl::NandLink;
use crate::bus::{Cx, Device};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

const REG_SRC: u64 = 0x00;
const REG_DST: u64 = 0x04;
const REG_LEN: u64 = 0x08;
const REG_CTRL: u64 = 0x0c;
const NAND_FIFO_ADDR: u64 = 0x2180_101c;
const IRQ_DELAY_TICKS: u64 = 50;

#[derive(Clone, Serialize, Deserialize)]
struct State {
    irq: u32,
    regs: BTreeMap<u64, u32>,
    next_irq_at: Option<u64>,
}

/// One channel of the SXC DMA controller.
pub struct DmaChannel {
    base: u64,
    core: usize,
    irq: u32,
    ram_base: u64,
    regs: BTreeMap<u64, u32>,
    next_irq_at: Option<u64>,
    nand: NandLink,
}

impl DmaChannel {
    /// Construct an SXC DMA channel at `base` wired to `core` / `irq`.
    pub fn new(base: u64, core: usize, irq: u32, ram_base: u64) -> Self {
        Self::with_nand(base, core, irq, ram_base, Arc::default())
    }

    /// Construct an SXC DMA channel sharing `nand` state with the NAND controller.
    pub fn with_nand(
        base: u64,
        core: usize,
        irq: u32,
        ram_base: u64,
        nand: NandLink,
    ) -> Self {
        Self {
            base,
            core,
            irq,
            ram_base,
            regs: BTreeMap::new(),
            next_irq_at: None,
            nand,
        }
    }

    fn translate_ram(&self, addr: u64) -> u64 {
        if addr < 0x1000_0000 {
            self.ram_base + addr
        } else {
            addr
        }
    }

    fn perform_transfer(&self, cx: &mut Cx<'_>) {
        let src = u64::from(*self.regs.get(&REG_SRC).unwrap_or(&0));
        let dst = u64::from(*self.regs.get(&REG_DST).unwrap_or(&0));
        let len = (*self.regs.get(&REG_LEN).unwrap_or(&0)) as usize;
        let ctrl = *self.regs.get(&REG_CTRL).unwrap_or(&0);
        if len == 0 || len > 0x10_0000 || (ctrl & 2) != 0 {
            return;
        }
        let target = self.translate_ram(dst);
        let mut buf = vec![0u8; len];
        if src == NAND_FIFO_ADDR {
            let page = self.nand.lock().map(|n| n.page_addr).unwrap_or(0);
            if len > 0x200 {
                match page {
                    0x3f => {
                        buf[0x200] = 0xc9;
                        buf[0x205] = 1;
                        buf[0x206] = 2;
                    }
                    0x40 | 0x80 => {
                        buf[0x200] = 0xbb;
                        buf[0x204] = 0;
                    }
                    _ => {
                        buf[0x200] = 0xff;
                    }
                }
            } else if len >= 0x24 {
                // Minimal @PEI partition table for sector reads.
                buf[0..4].copy_from_slice(&0x4945_5040u32.to_le_bytes());
                buf[8..12].copy_from_slice(&1u32.to_le_bytes());
                buf[12..16].copy_from_slice(&8u32.to_le_bytes());
                buf[28..32].copy_from_slice(&0x10u32.to_le_bytes());
                buf[32..36].copy_from_slice(&0x100u32.to_le_bytes());
            }
        }
        let _ = cx.ram_write(target, &buf);
    }
}

impl Device for DmaChannel {
    fn name(&self) -> &'static str {
        "sxc-dma-channel"
    }

    fn read(&mut self, _cx: &mut Cx<'_>, address: u64, _size: u8) -> u64 {
        let off = address - self.base;
        u64::from(*self.regs.get(&off).unwrap_or(&0))
    }

    fn write(&mut self, cx: &mut Cx<'_>, address: u64, _size: u8, value: u64) {
        let off = address - self.base;
        let mut v = value as u32;
        if off == REG_CTRL {
            self.regs.insert(off, v);
            self.perform_transfer(cx);
            v |= 1;
            let at = cx.now() + IRQ_DELAY_TICKS;
            self.next_irq_at = Some(at);
            cx.schedule(at, 1);
        }
        self.regs.insert(off, v);
    }

    fn event(&mut self, cx: &mut Cx<'_>, _token: u64) {
        self.next_irq_at = None;
        let _ = cx.raise_irq(self.core, self.irq);
    }

    fn next_deadline(&self, _now: u64) -> Option<u64> {
        self.next_irq_at
    }

    fn fingerprint(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(State {
            irq: self.irq,
            regs: self.regs.clone(),
            next_irq_at: self.next_irq_at,
        })
        .map_err(|e| e.to_string())
    }
}
