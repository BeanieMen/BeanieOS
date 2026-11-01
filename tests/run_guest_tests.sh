#!/bin/bash
# BeanieOS guest regression harness.
#
# Builds the kernel and the testinit userspace suite, packs a minimal boot
# image where testinit runs as /bin/init, boots it under QEMU, waits for the
# suite markers on the serial log, hard-kills the VM without any clean
# shutdown, then boots again and verifies the fsync'd file survived.
#
# Tool overrides (for user-local toolchains): QEMU, PARTED, MKFS_FAT, MMD,
# MCOPY, QEMU_EXTRA.
set -uo pipefail
cd "$(dirname "$0")/.."

QEMU=${QEMU:-qemu-system-x86_64}
PARTED=${PARTED:-parted}
MKFS_FAT=${MKFS_FAT:-mkfs.fat}
MMD=${MMD:-mmd}
MCOPY=${MCOPY:-mcopy}
QEMU_EXTRA=${QEMU_EXTRA:-}

OUT=out/test
IMG=$OUT/test.img
ESP_OFFSET=1048576
KERNEL_OFFSET=33554432

say() { echo "[guest-tests] $*"; }

make kernel || { say "FAIL: kernel build"; exit 1; }
make -C userspace bin/testinit || { say "FAIL: testinit build"; exit 1; }

mkdir -p "$OUT"
rm -f "$IMG"
truncate -s 128M "$IMG"
"$PARTED" -s "$IMG" mklabel gpt
"$PARTED" -s "$IMG" mkpart ESP fat32 1MiB 32MiB
"$PARTED" -s "$IMG" set 1 esp on
"$PARTED" -s "$IMG" mkpart BEANIEOS fat32 32MiB 100%
"$MKFS_FAT" -F 32 --offset 2048 --invariant "$IMG"
"$MKFS_FAT" -F 32 --offset 65536 --invariant "$IMG"
"$MMD" -i "$IMG@@$ESP_OFFSET" ::EFI
"$MMD" -i "$IMG@@$ESP_OFFSET" ::EFI/BOOT
"$MCOPY" -i "$IMG@@$ESP_OFFSET" limine/BOOTX64.EFI ::EFI/BOOT/BOOTX64.EFI
"$MCOPY" -i "$IMG@@$ESP_OFFSET" limine/limine.conf ::limine.conf
"$MMD" -i "$IMG@@$KERNEL_OFFSET" ::boot
"$MMD" -i "$IMG@@$KERNEL_OFFSET" ::bin
"$MCOPY" -i "$IMG@@$KERNEL_OFFSET" target/x86_64/release/beanieos ::boot/kernel
"$MCOPY" -i "$IMG@@$KERNEL_OFFSET" userspace/bin/testinit ::bin/init
cp OVMF_VARS.4m.fd "$OUT/OVMF_VARS.fd"

boot_and_wait() { # $1=log $2=marker1 $3=marker2 -> kills VM, returns 0 when both seen
  rm -f "$1"
  "$QEMU" \
    -machine q35 \
    -netdev user,id=n0 -device e1000,netdev=n0 \
    -audiodev none,id=snd0 -device AC97,audiodev=snd0 \
    -drive if=pflash,format=raw,readonly=on,file=OVMF_CODE.4m.fd \
    -drive if=pflash,format=raw,file="$OUT/OVMF_VARS.fd" \
    -drive format=raw,file="$IMG" \
    -m 512 \
    -display none \
    -serial "file:$1" \
    $QEMU_EXTRA &
  local qpid=$!
  local waited=0
  while [ $waited -lt 180 ]; do
    if [ -f "$1" ] && grep -q "$2" "$1" && grep -q "$3" "$1"; then
      kill -9 "$qpid" 2>/dev/null
      wait "$qpid" 2>/dev/null
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done
  kill -9 "$qpid" 2>/dev/null
  wait "$qpid" 2>/dev/null
  return 1
}

say "phase 1: full suites + durability write"
if ! boot_and_wait "$OUT/phase1.log" "SYNC_READY" "TESTS_DONE"; then
  say "FAIL: phase 1 timed out; last serial output:"
  tail -30 "$OUT/phase1.log" 2>/dev/null
  exit 1
fi
grep -E "^(ok|FAIL|skip|--|TESTINIT|SYNC_READY|TESTS_DONE)" "$OUT/phase1.log" | sed 's/\r//g'
if ! grep -q "TESTS_DONE rc=0" "$OUT/phase1.log"; then
  say "FAIL: guest reported failures (rc=1)"
  exit 1
fi
for marker in NET_DHCP_OK NET_DNS_OK NET_TIME_OK; do
  if ! grep -q "$marker" "$OUT/phase1.log"; then
    say "FAIL: missing network marker $marker (DHCP/DNS negotiation)"
    exit 1
  fi
done

say "phase 2: post-hardreset durability verify"
if ! boot_and_wait "$OUT/phase2.log" "PERSIST_VERIFY" "TESTS_DONE"; then
  say "FAIL: phase 2 timed out; last serial output:"
  tail -30 "$OUT/phase2.log" 2>/dev/null
  exit 1
fi
grep -E "^(PERSIST_VERIFY|TESTS_DONE)" "$OUT/phase2.log" | sed 's/\r//g'
if ! grep -q "PERSIST_VERIFY PASS" "$OUT/phase2.log"; then
  say "FAIL: fsync'd file did not survive a hard reset"
  exit 1
fi

say "PASS: all guest suites green, durability verified"
