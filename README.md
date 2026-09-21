# BeanieOS

BeanieOS is a hobby operating system with a rust kernel (yes we are ALL on the rust hype train because it so so much better). started this project by following [phillip](https://os.phil-opp.com/async-await/) and his amazing tutorial and then branched out. we have a website for this project under the docs folder

we wanna make it easier for future rust devs to make an os. there arent that many good tutorials and resources are lacking if you cant piece together the [osdev wiki](https://wiki.osdev.org/)

the docs + guide + download website will guide users on how to start on osdev and build on this os itself or they can just use an iso and run it (will make one after this is in a usable state)

we are still currrently in the ring 0 (kernel ring). this readme will be updated whenever me and pranjal make progress on this


# Demo
<img width="2880" height="1800" alt="image" src="https://github.com/user-attachments/assets/ebbefe71-3864-4fa5-b296-5402ee7dad83" />

<img width="2880" height="1800" alt="image" src="https://github.com/user-attachments/assets/0db947a8-66b3-4c51-a578-b283195dd930" />


# Features
The features/things for this operating system that will be implemented are

- multithreading (not started)
- interrupts (done)
- bootloader (done)
- fs (not started)
- networking (not started)
- a website (wip under docs)
- security switching (switching to ring 3, not strated)
- syscalls (not started)
- display output (we do get a proper display but standardization is required. wip)
- memory management (mostly done in the knowledge i know of)
- asynchrnous execution (done, base step for multithreading)
- MADT support via ACPI

ill add more features as i get to know about them from learning and pranjal will make everything related to ui for this operating system. (we are gonna ship this with so many custom apps this project is gonna be goated ash)

# Instructions

tho i would not recommend running this rn as it is just a black screen + some text + a red box (for testing the display)
if you do want to test anyways you can do 

```
make # sets up everything required
make run # builds an image with separate esp and runs the image with qemu
```

it might ask for installing toolchains. this project ONLY works with `cargo +nightly` so ig youre planning to run install all toolchains with cargo nightly please

# Contributors

## BeanieMan: Infra + OS 

BeanieMan is handling the os dev parts and writing the docs

## Pranjal: UI/UX + Art

Pranjal is handling everything related to art, frontend.


we both are going out of our comfort zones to learn something new so i hope thirdspace helps us in this journey
