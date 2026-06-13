SRC_DIR := src
BUILD_DIR := build

ASM := nasm
CC := gcc
LD := ld
OBJCOPY := objcopy
QEMU := qemu-system-i386

CFLAGS := -m32 -ffreestanding -fno-pie -fno-stack-protector -nostdlib -O0 -g
LDFLAGS := -m elf_i386 -T linker.ld

BOOT_SRC := $(SRC_DIR)/boot/boot.asm

C_SRCS := $(shell find $(SRC_DIR)/kernel -name '*.c')
ASM_SRCS := $(shell find $(SRC_DIR)/kernel -name '*.asm')

C_OBJS := $(patsubst $(SRC_DIR)/kernel/%.c,$(BUILD_DIR)/%.o,$(C_SRCS))
ASM_OBJS := $(patsubst $(SRC_DIR)/kernel/%.asm,$(BUILD_DIR)/%.o,$(ASM_SRCS))

OBJS := $(C_OBJS) $(ASM_OBJS)

all: $(BUILD_DIR)/os-image.bin

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/boot.bin: $(BOOT_SRC) | $(BUILD_DIR)
	$(ASM) -f bin $< -o $@

$(BUILD_DIR)/%.o: $(SRC_DIR)/kernel/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/%.o: $(SRC_DIR)/kernel/%.asm
	@mkdir -p $(dir $@)
	$(ASM) -f elf32 $< -o $@

$(BUILD_DIR)/kernel.elf: $(OBJS) linker.ld
	$(LD) $(LDFLAGS) $(OBJS) -o $@

$(BUILD_DIR)/kernel.bin: $(BUILD_DIR)/kernel.elf
	$(OBJCOPY) -O binary $< $@

$(BUILD_DIR)/os-image.bin: $(BUILD_DIR)/boot.bin $(BUILD_DIR)/kernel.bin
	cat $^ > $@

run: $(BUILD_DIR)/os-image.bin
	$(QEMU) -drive format=raw,file=$<

debug: $(BUILD_DIR)/os-image.bin
	$(QEMU) -s -S -drive format=raw,file=$<

clean:
	rm -rf $(BUILD_DIR)

.PHONY: all run debug clean