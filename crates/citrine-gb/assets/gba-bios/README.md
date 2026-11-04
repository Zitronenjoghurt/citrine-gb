# Built-in GBA BIOS

`cult-of-gba.bin` is the [Cult-of-GBA BIOS](https://github.com/Cult-of-GBA/BIOS) by DenSinH and fleroviux, MIT licensed (see `LICENSE`), built from commit `a30e9a96df083628b650724b7d4d7112b4070b98` with `changes.patch` applied.

The patch moves code to the official BIOS's addresses. The IRQ handler starts at 0x128 and the SWI handler at 0x140, the jump into the cartridge after the intro is at 0xDC, and the words at 0xE4 and 0x190 hold the opcodes the official BIOS has there. Reads from the BIOS while the CPU runs outside it return the last opcode the BIOS fetched, so with this layout they return the same values as with the official BIOS.

It also makes these calls match GBATEK:

- Div and DivArm give the remainder the numerator's sign.
- Sqrt(0) returns 0.
- RegisterRamReset clears the memory named by the highest flag set (OAM for 0x1F, EWRAM for 0x01).
- IntrWait with r0 = 0 returns at once when the IRQ was already flagged.
- The intro sets POSTFLG to 1.

And these match the official BIOS's behaviour, measured in our core:

- RegisterRamReset sets DISPCNT to 0x0080 and the low byte of SOUNDCNT_H to 0x0E, and leaves JOYCNT alone, so JOYCNT is 0 after the intro.
- SoundBias moves the level in steps of 2 towards 0 or 0x200 at the official pace (about 15,700 cycles for the whole ramp) and leaves r1 and r3 as the official one does.
- The RL decompressors run in Thumb, as in the official BIOS.
- LZ77, RL, Huffman and BitUnPack take about as many cycles per item as the official ones (within 30 cycles on the probe cases, Huffman and BitUnPack within 400), using waits where the loops differ.
- GetBiosChecksum reads its own 4096 words at the official pace before it returns 0xBAAE187F.
- The intro is as long as the official one and jumps into the cartridge on cycle 75,997,993 (line 126, dot 841), with the same registers, memory and timer 0 count.
- Its steps fall on the official intro's frames. RegisterRamReset runs first, then POSTFLG, IE and DISPSTAT are set and the sound registers get the official values. Timer 0 starts with reload 0xFB1A on line 159 of frame 6, one cycle earlier when bit 7 of header byte 0xB4 is set. The screen turns on in frame 6, fades in from white over frames 7 to 22 and fades to white over frames 217 to 248, and the intro waits for vblank in every frame up to frame 268. Timer 0 stops 6,183 cycles after that vblank, then the last RegisterRamReset runs.
- The official intro spends that time drawing the cartridge's logo, playing its sound and checking the link port for a multiboot host. This one does none of that. Counted waits (`boot_wait`) fill the time between its steps.

To rebuild it:

```sh
git clone https://github.com/Cult-of-GBA/BIOS && cd BIOS
git checkout a30e9a96df083628b650724b7d4d7112b4070b98
git apply ../changes.patch
arm-none-eabi-as entrypoint.s -mcpu=arm7tdmi -o entrypoint.o
arm-none-eabi-objcopy entrypoint.o bios.bin -O binary
sha256sum bios.bin
```

SHA-256: `bdc8a51380cc18a41ecb1f7140f5a8dcc23840b9ae93cd233e92c8aa2305cb53`
