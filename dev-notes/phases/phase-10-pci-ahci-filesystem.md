# Phase 10: PCI Bus Enumeration, AHCI (SATA) Driver, & FAT Filesystem Mount

## 🌟 High-Level Overview
A computer with only volatile RAM loses all its data the second the power switch is flipped. To store files, operating systems must talk to permanent storage drives (SATA hard disks or SSDs).

On modern PCs, storage controllers do not sit on fixed legacy ports. They are connected to the high-speed **PCI (Peripheral Component Interconnect)** bus.

In this phase, we:
1. Walk the **PCI Bus** to discover the storage controller.
2. Verify the device is a modern **AHCI (Advanced Host Controller Interface)** SATA controller.
3. Read the controller's **BAR5** register to locate its Memory-Mapped I/O (MMIO) controls.
4. Perform an **HBA hardware reset** and bring up the SATA communication line (PHY).
5. Build **DMA (Direct Memory Access)** command tables and issue `ATA_IDENTIFY` and `ATA_READ_DMA_EXT` commands.
6. Mount a standard **FAT Filesystem** so our kernel can open, read, and write files!

---

## 📖 Layman's Glossary: Jargon Demystified

*   **PCI (Peripheral Component Interconnect):**
    The universal internal highway that connects graphics cards, network cards, and storage controllers to the CPU.
*   **PCI Configuration Space (`0xCF8` & `0xCFC`):**
    To inspect PCI devices, x86 provides two legacy I/O ports:
    *   Port `0xCF8` (Address): You write the Bus, Device, and Function number you want to inspect.
    *   Port `0xCFC` (Data): You read or write the actual 32-bit register from that device.
*   **Class & Subclass Codes:**
    Every PCI device announces what kind of hardware it is:
    *   Class `0x01`: Mass Storage
    *   Subclass `0x06`: SATA Controller
    *   Interface `0x01`: AHCI Specification
*   **BAR (Base Address Register):**
    Registers in the PCI header where the device requests a chunk of memory. For AHCI controllers, **BAR5 (ABAR)** holds the physical MMIO address that controls all SATA ports.
*   **DMA (Direct Memory Access):**
    Instead of making the CPU manually copy every single byte from the disk using `in` and `out` instructions, the AHCI controller is an independent processor. You tell the controller: *"Copy sector 100 directly into RAM address `0x20_0000`"*, and the controller does it over the motherboard bus while the CPU does other work!
*   **PRDT (Physical Region Descriptor Table):**
    A list of memory addresses and byte lengths that tell the AHCI DMA engine where to put the data read from the disk.
*   **Sector (512 bytes):**
    The atomic unit of disk storage. You cannot read a single byte from a hard disk; you must read an entire 512-byte block at a time.
*   **Filesystem (FAT):**
    A hard drive is just a dumb sequence of 512-byte sectors. A filesystem introduces folders, filenames, creation dates, and tracks which sectors belong to which file.

---

## 🗺️ What Files are Involved?
1. [src/arch/pci.rs](file:///home/aj/BeanieOS/src/arch/pci.rs) — Scans the PCI bus and reads BAR5.
2. [src/arch/ahci.rs](file:///home/aj/BeanieOS/src/arch/ahci.rs) — Hand-crafted AHCI SATA driver conforming to the AHCI 1.3 spec.
3. [src/fs/mod.rs](file:///home/aj/BeanieOS/src/fs/mod.rs) — Block device adapter implementing the `fatfs` storage traits.
4. [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L97-L124) — `boot_disk()` orchestration.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: Scanning the PCI Bus
Located at lines 10–42 & 70–95 of [src/arch/pci.rs](file:///home/aj/BeanieOS/src/arch/pci.rs):

We loop through all possible buses (0–255), devices (0–31), and functions (0–7):
```rust
impl ConfigRegionAccess for PciConfig {
    unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
        let config_address = 0x8000_0000 // Enable bit
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);

        Port::<u32>::new(0xCF8).write(config_address);
        Port::<u32>::new(0xCFC).read()
    }
}
```
If `vendor_id != 0xFFFF`, a device is plugged in! We look for:
`class == 0x01 && subclass == 0x06 && interface == 0x01` (AHCI SATA).

---

### Step 2: Locating BAR5 & Global HBA Reset
Located at lines 104–109 of [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L104-L109) and lines 150–175 of [src/arch/ahci.rs](file:///home/aj/BeanieOS/src/arch/ahci.rs#L150-L175):

1.  We read **BAR5** from the PCI configuration space to get the AHCI controller's base MMIO register address.
2.  We perform a hardware reset:
    ```rust
    // Set bit 0 (HR = HBA Reset)
    write_volatile(ghc_ptr, read_volatile(ghc_ptr) | GHC_HR);
    // Wait until hardware clears the HR bit...
    while (read_volatile(ghc_ptr) & GHC_HR) != 0 {}
    
    // Set bit 31 (AE = AHCI Enable)
    write_volatile(ghc_ptr, read_volatile(ghc_ptr) | GHC_AE);
    ```

---

### Step 3: Checking SATA Physical Link (`PxSSTS`)
Located at lines 200–225 of [src/arch/ahci.rs](file:///home/aj/BeanieOS/src/arch/ahci.rs#L200-L225):

An AHCI controller can have up to 32 SATA ports. We inspect each port's `PxSSTS` (Serial ATA Status) register:
```rust
let ssts = read_volatile(port.add(PxSSTS));
let det = ssts & SSTS_DET_MASK;
if det == SSTS_DET_PHY_UP {
    // Port has an active drive plugged in and physical link established!
}
```

---

### Step 4: Configuring DMA Structures & Issuing Commands
Located at lines 230–350 of [src/arch/ahci.rs](file:///home/aj/BeanieOS/src/arch/ahci.rs#L230-L350):

To read from disk, we configure the hardware structures:
1.  **Command List:** 32 slots pointing to Command Tables.
2.  **Command Table:** Contains a **FIS (Frame Information Structure)** describing the ATA command, and a **PRDT** describing where in RAM to write the data.
3.  **Issuing `ATA_IDENTIFY` (Command `0xEC`):**
    We write to `PxCI` (Port Command Issue) register. The AHCI hardware automatically reads the sector info directly into `TRANSFER_BUFFER` via DMA. From this, we read sector size (512 bytes) and total sector count!

---

### Step 5: Mounting the FAT Filesystem
Located at lines 23–58 of [src/fs/mod.rs](file:///home/aj/BeanieOS/src/fs/mod.rs#L23-L58):

We wrap the controller in a `Disk` struct implementing `fatfs::Read`, `Write`, and `Seek`:
```rust
impl Read for Disk {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        let at = self.at();
        self.controller.read_at(at, buffer)?;
        self.position += buffer.len() as u64;
        Ok(buffer.len())
    }
}
```
In `src/fs/mod.rs`, it reads the MBR partition table, detects the first FAT partition, and passes it to `fatfs::FileSystem::new(disk, FsOptions::new())`. The filesystem is now live!

---

## 🎯 Summary Checklist
By the end of Phase 10, our operating system has:
1. Probed the PCI bus via ports `0xCF8` and `0xCFC`.
2. Located an AHCI SATA controller and extracted its BAR5 MMIO address.
3. Performed a hardware HBA reset and verified the SATA physical PHY link.
4. Set up Command Lists, Command Tables, and PRDT buffers for bus-mastering DMA.
5. Issued `ATA_IDENTIFY` and disk read commands.
6. Mounted a FAT filesystem capable of file I/O operations.
