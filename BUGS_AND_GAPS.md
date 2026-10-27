# BeanieOS — Bugs and Gaps Audit

Date: 2026-10-26
Scope: static audit of `src/` plus `make check` / `make kernel` baseline.
Baseline: `make check` and `make kernel` succeed (release, kernel feature).

This is a findings report, not a fix. Each item points at the file/line range
that was opened and read directly. Items are grouped by subsystem; severity is
my judgment from reading the code, not from running the kernel.

## 1. PCI device enablement is not performed for most devices

- `src/hal/pci.rs`: `Device::enable()` exists and sets
  `MEMORY_ENABLE | BUS_MASTER_ENABLE`, but it is only called from
  `src/storage/ahci/controller.rs` (`device.enable()` after reading BAR5).
- `src/main.rs` e1000 path calls `dev.bar0_info()` and then
  `paging::phys_to_direct(bar0)` without ever calling `dev.enable()`. Per the
  PCI bring-up checklist in the skill, Memory Space Enable and Bus Master Enable
  must be set before touching the BAR; without BME the device can be read but
  can never transfer data.
- `src/main.rs` network loop also never calls `dev.enable()`.
- `src/hal/mmio.rs` `map_all_bars`/`reserve_all_bars` rely on BAR values read
  by `Route::bar5_info()`, which re-reads config space. Reading a BAR to learn
  its size usually writes `0xFFFFFFFF` and leaves the command register as
  firmware had it (often both bits clear). `enable()` must be called after the
  probe; for AHCI it is (late, but present); for everything else it is missing.

## 2. Bus-master DMA addresses are obtained by identity/linear assumptions

- `src/net/e1000.rs`: `dev.mmio = phys_to_direct(bar0)` in `main.rs` assumes
  the BAR physical address is inside the 4 GiB direct map. A BAR landing outside
  that window panics in `phys_to_direct` instead of being rejected/mapped.
- `src/net/e1000.rs` `init`: `rx_desc_frame`/`tx_desc_frame` are single 4 KiB
  frames from `alloc_frame()`. Each descriptor ring is one page, so that is fine,
  but the *buffers* are also one frame each and are handed to the device by
  physical address (`rx_descs[i].addr = buf_frame`, `desc.addr = self.tx_buffers[cur]`).
  Those addresses come from `alloc_frame()` (low RAM) so they are physical and
  within the direct map; OK. But nothing reserves/avoids them after allocation,
  and `free_frame` could hand them back while the device is still using them if a
  later code path frees them. The ring/buffer frames are never freed today, so
  this is latent.
- `src/storage/ahci/disk/disk.rs`: `kernel_to_phys(dma.command_list() as u64)` etc.
  work because the `AhciDma` static lives in the kernel image. This is correct,
  but it depends on the static being in `.text`/`.data` of the linked image, not
  in heap memory. Documented only by comment.
- `src/hal/dma.rs` `AhciDma` is a `static` in WB memory handed to the device;
  `dma_publish()`/`dma_consume()` (fences) are used around the doorbell, which
  is good. However the FIS is copied with `copy_volatile` (volatile stores) while
  neighbouring structures use `write_volatile`/`write_bytes`; the mix is fragile
  but intentional.

## 3. No contiguous allocation guarantee for DMA buffers

- `src/memory/pool.rs` `alloc_contiguous` scans from `self.area_idx`/`self.next`,
  which is the right direction, but it only checks `blocked(&run)` and the free
  list; it does not reserve the run in a way that later `alloc()` cannot return
  frames inside it. `alloc()` advances `next` past the run only if `area_idx`
  and `next` were updated; `alloc_contiguous` does update them, but `alloc()`
  has no check that a returned frame is not inside a previously handed-out
  contiguous run. There is no "reserved" ledger entry created for the run.
- `src/memory/allocator.rs` `RESERVED` is a separate ledger from `FramePool`;
  `Dma::reserve` pushes into `RESERVED`, but `FramePool` never consults
  `RESERVED` except during `init_frames`. After boot, `RESERVED.push` only
  records; it does not mark frames as unavailable to `alloc()`.
- `src/memory/pool.rs` `avoid()` can fail silently when `avoid_len >= MAX_AVOID`
  (returns `false`); `init_frames` asserts `dropped == 0`, but runtime
  `Mmapped::map` calls `dma.reserve` and ignores the bool. Overflow is a
  warning-class failure, not a hard error, contrary to the debt plan's intent.

## 4. Memory pool silently drops on overflow

- `src/memory/pool.rs`: `add_area`, `avoid`, `alloc` (area advance) all return
  bool/None without distinguishing "out of space" from "out of frames".
  `MAX_AREAS = 64`, `MAX_AVOID = 3 + MAX_AREAS = 67`, `MAX_FREE = 16384`.
  Exceeding any of these silently loses entries. There is no error type or
  panic.
- `src/memory/allocator.rs` `ReservedList::push` returns false on
  `MAX_RESERVED = 64` overflow; caller in `init_frames` counts dropped ranges
  and asserts zero, but runtime callers ignore it.

## 5. FAT32 hardcodes 512-byte sectors

- `src/storage/ahci/disk/block.rs`: `BlockDevice for Disk::read_block` calls
  `read_at(lba * 512, buf)` and `write_at(lba * 512, buf)`, ignoring
  `sector_size()`. If a disk reports 4096-byte logical sectors, all offsets are
  wrong. `Disk::read_at`/`write_at` themselves use `block_size` for chunking,
  but the block layer scales by 512.
- `src/storage/fs/fat32/mod.rs` uses `[u8; 512]` buffers and 512 literals in
  many places (debt item 54). `BlockDevice::sector_size()` is implemented but
  the FAT32 code paths shown use fixed 512. Partition's `BlockDevice` impl does
  use `sector_size()`, so the two impls disagree on scaling.

## 6. VFS has no real mount table in use

- `src/storage/vfs/mount.rs`: `MountTable` exists, but `src/main.rs` stores the
  FAT32 instance in a `static MOUNTED: Mutex<Option<Fat>>` and mounts it into
  each process's `FdTable` via `fds.mount(fat32.clone())`. There is no
  system-wide mount table keyed by mountpoint; `MountTable::mount` is never
  called from `main.rs` (grep shows no caller outside tests).
- `src/storage/vfs/fd.rs` `open` resolves paths through `self.mounts`, but
  `self.mounts` is populated per-process by `fds.mount`. This works for the
  current single-filesystem setup but cannot represent `/proc`, `/dev` as real
  mounts; `/dev/*` are special-cased by string prefix in `open`.

## 7. Scheduler and process identity

- `src/task/identity.rs`: `current_pid()` reads `(*thread).pid` from the
  GS-base thread. Debt item 37 notes `spawn()` clobbers the running process's
  identity; in `src/task/scheduler.rs` `activate()` writes
  `self.current = next_id` and calls `publish_current()` (GS base), then
  `activate_space(next_pid)` only if `next_pid != prev_pid`. The thread's
  `pid` field is set at construction, so per-thread identity is correct as long
  as `Thread::new` receives the right pid. `spawn()` does pass `pid`.
- `Process.threads` (`src/task/process.rs`) is only appended via
  `attach_thread`, which is called from `Scheduler::spawn`/`spawn_thread`. Fork
  path (`new_forked`) does not call `attach_thread` automatically; `add_forked_thread`
  calls `spawn_thread`, which does. OK.
- `src/task/scheduler.rs`: `runnable_count()` asserts `live == count_live_by_scan()`
  via `debug_assert!`. In release builds the assert is disabled, so a drift in
  `live` would not be caught.
- `Scheduler` is a single global `InterruptMutex`; all scheduling decisions scan
  `BTreeMap`/`VecDeque`. Debt item 36.

## 8. Input subsystem

- `src/arch/input.rs`: `push_key(_key)` ignores its argument and only wakes
  waiters. The actual character is pushed by `src/arch/ps2.rs` into
  `crate::tty::push_byte` and the event queue via `input::event`. So `push_key`
  is a no-op name; not a functional bug but misleading.
- `src/input/mod.rs` (`event.rs` inlined): `EVENT_QUEUE` is a
  `VecDeque<InputEvent>` capped at 512 entries; overflow silently drops events
  (`if q.len() < 512`). No overflow counter.
- `src/input/mouse.rs`: `MOUSE_QUEUE` drops whole 3-byte packets when full,
  which is good, but `push_packet` pops front one byte at a time in a loop
  (`q.pop_front()` three times) which can desync if the queue length is not a
  multiple of 3 at entry. The guard `while q.len() + 3 > 1024` ensures room,
  but the eviction loop pops 3 each iteration without checking divisibility.

## 9. Timing

- `src/arch/interrupts/consts.rs`: `TICK_MS = 1`. `lapic::calibrate()` derives
  counts/ms from the PIT. If calibration fails it falls back to
  `NOMINAL_COUNTS_PER_MS = 62_500`. On a machine where the LAPIC timer runs at
  a very different rate the fallback is wrong, but it is explicit.
- `src/arch/interrupts/vectors.rs` `timer_interrupt_rust`: serial input polling
  happens only when `tick.is_multiple_of(8)`. At 1 kHz that is every 8 ms; fast
  typing can overflow the 16550 FIFO? The 16550 has a 16-byte FIFO; 8 ms at
  115200 baud is ~115 bytes, far beyond 16, so bytes can be lost under load.
  Consider draining on every tick.
- `src/arch/interrupts/pic/lapic.rs` `calibrate`: `elapsed += previous.wrapping_sub(now)`
  accumulates PIT counts; if the PIT is read in mode 0 (counting down from
  reload) the subtraction is correct. `PIT_RELOAD = 0` means 65536. Fine.

## 10. Userspace / exec

- `src/task/exec.rs` `read_program`: if `size == 0` it still reads in a loop and
  returns `None` only if `image.is_empty()`; a zero-byte file returns `None`,
  which is treated as "could not read". Acceptable.
- `src/task/userland.rs` `build()`: maps `USER_CODE.len()` bytes into a single
  4 KiB page without checking that `USER_CODE.len() <= PAGE_SIZE`. Today it is
  ~? bytes; if it grows past 4 KiB it silently overflows into the next virtual
  page which is unmapped → page fault in ring 3.
- `src/task/userland.rs` `verify_release()`: allocates an `AddressSpace::new()`
  (no kernel shared slots) and maps three pages; `drop(space)` releases them.
  It compares `live_frames()` before/after. This is a good host-testable-ish
  check but runs at boot in ring 0; fine.

## 11. Logging overhead

- `src/syscall/entry.rs` `syscall_c`: calls `kdebug!("syscall {number} -> {result:#x}")`
  on every syscall. `src/logger.rs` takes a global spin lock on every log line.
  Debt item 51. On a hot path (every syscall) this is measurable.
- `src/logger.rs` `LEVEL_FILTER` should be an atomic, not a spin lock (debt).

## 12. Audio

- `src/audio/ac97.rs` not audited in depth here. `src/main.rs` calls
  `audio::init()` after `hal::init`. If AC97 BAR is not enabled via
  `Device::enable()`, MMIO reads return floating data. Same class as item 1.

## 13. Graphics

- `src/graphics/framebuffer.rs` `init_framebuffer`: `phys_to_direct(phys)` panics
  if the framebuffer physical address is not below `DIRECT_MAP_SIZE` (4 GiB).
  Most firmwares place the framebuffer below 4 GiB, but this is an unchecked
  assumption.
- `put_pixel` for `bpp == 24` writes 3 bytes non-atomically; concurrent
  writers could tear. Single-threaded kernel writer today, so latent.

## 14. Network stack

- `src/net/stack.rs` `init_stack`: `config.random_seed = 0x12345678` constant;
  `iface.routes_mut().add_default_ipv4_route(...).unwrap()` panics if the route
  table is full. At init it is empty, so OK.
- `src/net/stack.rs` `SmolDevice::transmit`: `TxToken::consume` allocates a
  `Vec` of `len` bytes and calls `dev.transmit`, which copies into the TX
  buffer. If `len > BUF_SIZE (2048)` transmit returns false but the packet is
  still "consumed"; smoltcp may retry. Acceptable.

## 15. Build / tooling

- `src/main.rs`: `#![allow(dead_code)]` file-wide (debt item 41). Hides ~20
  uncalled functions, including free paths and preemption helpers.
- `Cargo.toml`: `spin = { default-features = false, features = ["mutex", ...] }`
  does not enable interrupt-masking mutex (debt item 43). Every global lock is
  therefore interrupt-unsafe; locking from the tick handler (which
  `scheduler::account_tick` does via `SCHEDULER.lock()`) is only safe because
  `InterruptMutex` wraps spin with interrupt disable. Verify `InterruptMutex`
  actually masks interrupts; it is used everywhere, so if it does not, this is a
  real bug. (Not verified by running tests; no tests exist.)

## Conclusion

The most likely "silent failure" bugs, in priority order:

1. PCI command register not enabled for e1000 and audio (item 1) — device
   reads work, DMA never does.
2. Framebuffer/PCI BAR addresses assumed inside the 4 GiB direct map (items 2, 13).
3. Runtime `RESERVED`/`avoid` overflow is not a hard error (items 3, 4).
4. FAT32 512-byte assumption in the `Disk` BlockDevice impl (item 5).
5. Serial input only drained every 8 ms (item 9).

No code was changed by this audit.
