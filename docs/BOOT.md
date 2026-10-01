# Boot protocol

How the BIOS ([`bios/bios.tri`](../bios/bios.tri)) starts the operating system.

## 1. Reset

The CPU resets in ring +1 with paging and interrupts off, at `PC = −59 049`
(ISA §9). That address holds the BIOS, compiled with `trine` using the BIOS
memory layout:

| Frame | Physical addresses | Used for |
|---|---|---|
| −3 … −1 | −68 890 … −9 842 | BIOS code and strings (ROM) |
| 0 | −9 841 … 9 841 | free (the RAM self-test writes here) |
| **1** | **9 842 … 29 524** | **the boot sector** |
| 2 | 29 525 … 49 207 | BIOS stack (top at 49 206) |
| 3 … | 49 208 … | BIOS globals |

## 2. The BIOS

1. **Machine check.** The BIOS reads the RAM size from `SYSCTL` and tests frame 0 and the last frame (write, read back, restore). It reads the time from the RTC and asks both disks for their size.
2. **Info screen.** It shows the results on the text screen and copies them to the serial console.
3. **Boot.**
   - It reads **disk 0, sector 0** (one page, 3^9 trytes) by DMA into **frame 1**.
   - It checks that the **last word of frame 1** (address 29 523) holds the **boot signature `0zCAFEBABE`** (31 857 270 899).
   - It **jumps to the centre of frame 1, address 19 683**, with:
     - `r1` = the boot disk number (0)
     - `r2` = the number of RAM pages
     - still in ring +1, with paging and interrupts off
4. **Monitor.** If there is no disk, the read fails, or the signature is missing, the BIOS says why and starts a monitor on the keyboard and serial console:

| Command | Effect |
|---|---|
| `help` | list the commands |
| `info` | CPU, RAM, clock and disks |
| `time` | the clock (UTC) |
| `dump A [N]` | N words from word address A, in base 27 and decimal |
| `boot` | try disk 0 again |
| `reset` / `off` | restart / power off |

## 3. The boot sector

| Sector trytes | Address | Contents |
|---|---|---|
| 0 … 9 839 | 9 842 … 19 681 | free for the boot program's data |
| 9 840 … 19 679 | 19 682 … 29 521 | code, entered at 19 683 |
| 19 680 … 19 682 | 29 522 … 29 524 | the signature word |

Code can also start lower, as long as execution begins at 19 683.

The boot program may use all of RAM. The BIOS never runs again after the jump, though its interrupt table pointer stays set until the program changes it.

## 4. Writing a boot program in Trine

```
fn main(disk: int, ram: int) -> int { ... }
```

```
trine build --boot prog.tri -o prog.disk    # one-sector disk image with the signature
trine run --boot prog.tri                   # boot it through the BIOS, headless
setun --disk prog.disk                      # boot it in a window
```

`trine build --boot` places the startup code at 19 683, the stack in frame 2 and the globals from frame 3. Its startup code passes `r1` and `r2` to `main` as `disk` and `ram`. [`bios/boot_hello.tri`](../bios/boot_hello.tri) is a complete example.

A one-sector program holds about 3 280 words. The next stage, a real loader, will read the kernel from further sectors.
