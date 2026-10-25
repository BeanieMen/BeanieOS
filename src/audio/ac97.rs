use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use x86_64::instructions::port::Port;

use crate::{hal::pci::Device, kinfo};

const AC97_RESET: u16 = 0x00;
const AC97_MASTER_VOL: u16 = 0x02;
const AC97_PCM_OUT_VOL: u16 = 0x18;
const AC97_EXT_AUDIO_STATUS: u16 = 0x2A;
const AC97_PCM_FRONT_DAC_RATE: u16 = 0x2C;

const AC97_PCM_OUT_BDBAR: u16 = 0x10;
const AC97_PCM_OUT_LVI: u16 = 0x15;
const AC97_PCM_OUT_SR: u16 = 0x16;
const AC97_PCM_OUT_CR: u16 = 0x1B;

const CR_RUN: u8 = 0x01;
const CR_RESET: u8 = 0x02;

const NUM_BUFFERS: usize = 32;
const BUFFER_SIZE: usize = 4096;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct BufferDescriptor {
    pub phys_addr: u32,
    pub samples: u16,
    pub flags: u16,
}

pub struct Ac97 {
    pub nambar: u16,
    pub nabmbar: u16,
    pub bdl_phys: u32,
    pub bdl_virt: *mut BufferDescriptor,
    pub buf_virt: [*mut u8; NUM_BUFFERS],
    pub cur_lvi: usize,
    pub pending: Vec<u8>,
}

unsafe impl Send for Ac97 {}
unsafe impl Sync for Ac97 {}

static INITIALIZED: AtomicBool = AtomicBool::new(false);

impl Ac97 {
    pub fn init(device: &Device) -> Option<Self> {
        device.enable_io_bus_master();
        let nambar = device.io_bar(0)?;
        let nabmbar = device.io_bar(1)?;
        kinfo!("ac97: nambar = {:#x}, nabmbar = {:#x}", nambar, nabmbar);

        // Allocate 1 physical frame for the Buffer Descriptor List (BDL)
        let bdl_frame = crate::memory::allocator::alloc_frame()?;
        let bdl_phys = bdl_frame.start_address().as_u64() as u32;
        let bdl_virt = crate::paging::phys_to_direct(bdl_phys as u64) as *mut BufferDescriptor;

        // Allocate physical frames for ring buffers (4096 bytes each = 1024 16-bit stereo samples)
        let mut buf_virt = [core::ptr::null_mut(); NUM_BUFFERS];
        for i in 0..NUM_BUFFERS {
            let frame = crate::memory::allocator::alloc_frame()?;
            let phys = frame.start_address().as_u64() as u32;
            let virt = crate::paging::phys_to_direct(phys as u64) as *mut u8;
            unsafe {
                core::ptr::write_bytes(virt, 0, BUFFER_SIZE);
                bdl_virt.add(i).write(BufferDescriptor {
                    phys_addr: phys,
                    samples: (BUFFER_SIZE / 2) as u16,
                    flags: 0,
                });
            }
            buf_virt[i] = virt;
        }

        // Reset Codec
        unsafe {
            Port::<u16>::new(nambar + AC97_RESET).write(1);
            // Master Volume: 0 dB (unmuted)
            Port::<u16>::new(nambar + AC97_MASTER_VOL).write(0x0000);
            // PCM Out Volume: 0 dB (unmuted)
            Port::<u16>::new(nambar + AC97_PCM_OUT_VOL).write(0x0000);
            // Enable Variable Rate Audio (VRA)
            let mut ext_status = Port::<u16>::new(nambar + AC97_EXT_AUDIO_STATUS);
            let val = ext_status.read();
            ext_status.write(val | 1);
            // Set Sample Rate to 44.1 kHz
            Port::<u16>::new(nambar + AC97_PCM_FRONT_DAC_RATE).write(44100);

            // Reset DMA PCM-out channel
            let mut cr = Port::<u8>::new(nabmbar + AC97_PCM_OUT_CR);
            cr.write(CR_RESET);
            while (cr.read() & CR_RESET) != 0 {}

            // Set BDL Physical Address
            Port::<u32>::new(nabmbar + AC97_PCM_OUT_BDBAR).write(bdl_phys);
            // Initialize LVI to the last buffer so it plays silence on startup and halts cleanly
            Port::<u8>::new(nabmbar + AC97_PCM_OUT_LVI).write((NUM_BUFFERS - 1) as u8);
            // Start DMA playback (plays initial silence and halts at NUM_BUFFERS - 1)
            cr.write(CR_RUN);
        }

        kinfo!("ac97: DMA audio playback initialized (44.1kHz 16-bit stereo)");
        INITIALIZED.store(true, Ordering::Relaxed);

        Some(Self {
            nambar,
            nabmbar,
            bdl_phys,
            bdl_virt,
            buf_virt,
            cur_lvi: NUM_BUFFERS - 1,
            pending: Vec::with_capacity(BUFFER_SIZE * 4),
        })
    }

    pub(crate) fn write_samples(&mut self, data: &[u8]) -> usize {
        self.flush();
        const MAX_PENDING: usize = BUFFER_SIZE * 8;
        if self.pending.len() >= MAX_PENDING {
            return 0;
        }
        let space = MAX_PENDING - self.pending.len();
        let to_take = data.len().min(space);
        self.pending.extend_from_slice(&data[..to_take]);
        self.flush();
        to_take
    }

    fn flush(&mut self) {
        let civ = unsafe { Port::<u8>::new(self.nabmbar + 0x14).read() as usize % NUM_BUFFERS };
        let sr = unsafe { Port::<u16>::new(self.nabmbar + AC97_PCM_OUT_SR).read() };
        let mut is_halted = (sr & 1) != 0;

        while self.pending.len() >= BUFFER_SIZE {
            let next_lvi = (self.cur_lvi + 1) % NUM_BUFFERS;
            if !is_halted && next_lvi == civ {
                break;
            }

            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.pending.as_ptr(),
                    self.buf_virt[next_lvi],
                    BUFFER_SIZE,
                );
            }
            self.pending.drain(0..BUFFER_SIZE);
            self.cur_lvi = next_lvi;

            unsafe {
                Port::<u16>::new(self.nabmbar + AC97_PCM_OUT_SR).write(0x1C);
                Port::<u8>::new(self.nabmbar + AC97_PCM_OUT_LVI).write(self.cur_lvi as u8);
                Port::<u8>::new(self.nabmbar + AC97_PCM_OUT_CR).write(CR_RUN);
            }

            is_halted = false;
        }
    }
}

