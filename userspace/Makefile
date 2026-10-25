CC ?= musl-gcc
CFLAGS := -Wall -Wextra -O2 -static -Iinclude -m64
LDFLAGS := -static

BUILD_DIR := bin

C_SRCS := $(sort $(wildcard src/*.c))
BINS := $(patsubst src/%.c,$(BUILD_DIR)/%,$(C_SRCS))

all: $(BINS) doom xfbdev

doom:
	@if [ -d doom/doomgeneric ]; then \
		$(MAKE) -C doom/doomgeneric; \
	fi

xfbdev:
	@if [ -d tinyxserver ]; then \
		$(MAKE) -C tinyxserver Xfbdev; \
		mkdir -p $(BUILD_DIR); \
		cp tinyxserver/Xfbdev $(BUILD_DIR)/Xfbdev; \
	fi

$(BUILD_DIR)/%: src/%.c
	@mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $(LDFLAGS) $< -o $@

clean:
	rm -f $(BINS) $(BUILD_DIR)/doom $(BUILD_DIR)/Xfbdev
	@if [ -d doom/doomgeneric ]; then \
		$(MAKE) -C doom/doomgeneric clean; \
	fi
	@if [ -d tinyxserver ]; then \
		$(MAKE) -C tinyxserver clean; \
	fi

.PHONY: all clean doom xfbdev