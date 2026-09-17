.PHONY: setup kernel boot bootloader-image kernel-image iso run-bootloader run clean

KERNEL_TARGET := x86_64-unknown-none
BOOT_TARGET := x86_64-unknown-uefi

KERNEL := target/$(KERNEL_TARGET)/debug/beanieos
BOOTLOADER := target/$(BOOT_TARGET)/debug/bootloader.efi

BOOTLOADER_IMG := target/bootloader.img
KERNEL_IMG := target/beanieos.img

OVMF_CODE := OVMF_CODE.4m.fd
OVMF_VARS := OVMF_VARS.4m.fd
OVMF_RUN_VARS := target/OVMF_VARS.fd

EFI_DIR := target/efi/EFI/BOOT
EFI_BOOT := $(EFI_DIR)/BOOTX64.EFI


setup:
	rustup default stable
	rustup toolchain install nightly
	rustup +nightly component add llvm-tools-preview
	yay -S --needed qemu-system-x86 qemu-desktop dosfstools mtools parted


kernel:
	cargo +nightly build \
		-Zbuild-std=core,alloc \
		-Zjson-target-spec \
		--target ./x86_64.json


boot:
	cargo build \
		--manifest-path bootloader/Cargo.toml \
		--target $(BOOT_TARGET)


bootloader-image: boot
	rm -f $(BOOTLOADER_IMG)

	truncate -s 64M $(BOOTLOADER_IMG)
	mkfs.fat -F 32 $(BOOTLOADER_IMG)

	mmd -i $(BOOTLOADER_IMG) ::EFI
	mmd -i $(BOOTLOADER_IMG) ::EFI/BOOT

	mcopy -i $(BOOTLOADER_IMG) \
		$(BOOTLOADER) \
		::EFI/BOOT/BOOTX64.EFI


iso: boot kernel
	rm -f $(KERNEL_IMG)

	truncate -s 64M $(KERNEL_IMG)
	mkfs.fat -F 32 $(KERNEL_IMG)

	mmd -i $(KERNEL_IMG) ::EFI
	mmd -i $(KERNEL_IMG) ::EFI/BOOT

	mcopy -i $(KERNEL_IMG) \
		$(BOOTLOADER) \
		::EFI/BOOT/BOOTX64.EFI

	mcopy -i $(KERNEL_IMG) \
		$(KERNEL) \
		::kernel


run-bootloader: bootloader-image
	mkdir -p target
	cp $(OVMF_VARS) $(OVMF_RUN_VARS)

	qemu-system-x86_64 \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive if=pflash,format=raw,file=$(OVMF_RUN_VARS) \
		-drive format=raw,file=$(BOOTLOADER_IMG) \
		-display sdl


run: iso
	mkdir -p target
	cp $(OVMF_VARS) $(OVMF_RUN_VARS)

	qemu-system-x86_64 \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive if=pflash,format=raw,file=$(OVMF_RUN_VARS) \
		-drive format=raw,file=$(KERNEL_IMG) \
		-display sdl


clean:
	cargo clean
	rm -f $(BOOTLOADER_IMG)
	rm -f $(KERNEL_IMG)
	rm -f $(OVMF_RUN_VARS)
	rm -rf target/efi