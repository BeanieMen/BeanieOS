### 14-09-2026 22:52

Hi Lol (Making my contribution to this project)

### 15-09-2026 20:46

we finally got a framebuffer meow

### 15-09-2026 22:28

set up the tss, gdt, idt, ist + add exception handlers for faults/double fault
this was lowkey confusing seeing how the segments, descriptors and tables came into play with each other

thinking of leaving this as is and adding some sort of bootloader (multiboot2). it could either be limine or grub

i dont want a 80x25 vga framebuffer i want a real one

ts frying me there is so much to learn

### 16-09-2026 20:52

i mean it has gotten a lot easier now we just setup interrupts. we get interrupt index from pic and set handler of that interrupts index from the pic with a handler function 

i also set up paging (already enabled via bootloader crate. will go into detail in the guide that me and nilu will make). we get bootinfo and from bootinfo, the physical memory offset and using that get the virt addr of page tables (physical addr in cr3 reg)

ig we can take input and give output rn. probably a smiple shell is possible but no filesystem or read or write is available to files only with the framebuffer

### 17-09-2026 17:07

i switched over to cachyos and setup everything again. setting this up made me realize the lack of automated setup for this project so i edited the makefile