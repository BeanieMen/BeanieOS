.PHONY: setup build run clean

setup:
	rustup default stable
	rustup toolchain install nightly
	rustup +nightly component add llvm-tools-preview
	cargo install bootimage
	yay -S --needed qemu-system-x86 qemu-desktop

build:
	cargo +nightly bootimage -Zbuild-std=core,alloc -Zjson-target-spec --target ./x86_64.json

run: build
	qemu-system-x86_64 \
		-drive format=raw,file=target/x86_64/debug/bootimage-beanieos.bin \
		-display sdl

clean:
	cargo clean