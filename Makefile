build:
	cargo +nightly bootimage -Zbuild-std=core -Zjson-target-spec --target ./x86_64.json

run: build
	qemu-system-x86_64 -drive format=raw,file=target/x86_64/debug/bootimage-beanieos.bin