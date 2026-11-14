# DS save types

`saves.bin` lists the save chip of 6,771 DS games by game code, since a DS header doesn't name its chip. Each entry is 5 bytes: the game code as stored in the header (4 bytes), then the chip (1 byte: 1 to 4 EEPROM 512 B, 8 KB, 64 KB, 128 KB; 5 to 7 FLASH 256 KB, 512 KB, 1 MB; 8 the 8 MB NAND chip; 0 no save). Entries are sorted by game code.

The table is SkyEmu's `src/nds_rom_database.h`, adapted from melonDS's game list and relicensed by the melonDS team under the MIT licence for SkyEmu (see `LICENSE`). Seven entries whose chip has no type here yet (the 16 MB and 64 MB NAND chips, and three marked unknown) are left out. To build it again:

```sh
curl -fsSLO https://raw.githubusercontent.com/skylersaleh/SkyEmu/main/src/nds_rom_database.h
python3 - <<'EOF2'
import re, struct
src = open("nds_rom_database.h").read()
rows = re.findall(r"\{0x([0-9A-Fa-f]{8}), 0x([0-9A-Fa-f]{8}), 0x([0-9A-Fa-f]{8})\}", src)
rows = sorted((int(code, 16), int(chip, 16)) for code, _, chip in rows if int(chip, 16) <= 8)
open("saves.bin", "wb").write(b"".join(struct.pack("<IB", code, chip) for code, chip in rows))
EOF2
sha256sum nds_rom_database.h saves.bin
```

SHA-256:

- `nds_rom_database.h`: a6bdcb9f3253cba2e08ea20c537a5369c81f8706ef7fc2804c5cc2e40b4a6943
- `saves.bin`: 82ef0474ad7fcca6d6280ab07655f8e1e961baef8bf2564b1b2f84b15e2435f1
