# BeanWM on BeanieOS

## Goal

Run BeanWM v2 as a static, x86-64-baseline userspace program on BeanieOS with TinyXfbdev providing the X server.

## Checkpoints

- [x] Repair userspace polling, device readiness, Doom startup, framebuffer validation, and allocator safety.
- [x] Implement the IPv4 socket operations required by TinyXfbdev: `listen()` and `getsockname()`.
- [x] Enable TinyX PanoramiX/Xinerama server support with server-private declarations.
- [x] Prevent ring-3 CPU faults from panicking the kernel.
- [x] Verify Xfbdev starts and `xhello` completes the X11 setup handshake in QEMU.
- [ ] Build a target-compatible C++ runtime. The host static `libstdc++` is glibc-built and emits unsupported AVX/V3 instructions.
- [ ] Build or provide musl-compatible static libstdc++, libsupc++, and required compiler support.
- [ ] Build static X11 client dependencies with one consistent feature configuration: XCB, X11, Xext, Xinerama, Xau, and Xdmcp.
- [ ] Add a reproducible BeanWM source/build target under `userspace/beanwm`.
- [ ] Link BeanWM with `-march=x86-64 -mtune=generic` and verify no dynamic interpreter or unsupported ISA notes.
- [ ] Package BeanWM and its default configuration into the FAT image.
- [ ] Run Xfbdev, launch BeanWM, and verify X11 manager startup in QEMU.
- [ ] Test BeanWM window management with a small X client and verify shutdown/restart behavior.
- [ ] Commit each verified phase separately.

## Validation Gates

Each phase must pass its narrow check before the next commit:

```text
make check
make userbins
make disk
QEMU_DISPLAY=none make run
run Xfbdev &
run xhello
run beanwm
```

For the final binary, also require:

```text
file userspace/bin/beanwm
readelf -l userspace/bin/beanwm
readelf -n userspace/bin/beanwm
```

The final ELF must be static, have no `INTERP` segment, and require only x86-64-baseline instructions.

## Current Blocker

BeanWM links against X11/Xinerama and C++ standard-library facilities. Static X11 archives have been prototyped successfully. The remaining blocker is a C++ runtime built against musl; the installed static libstdc++ belongs to glibc and pulls in unsupported host ISA/runtime code.
