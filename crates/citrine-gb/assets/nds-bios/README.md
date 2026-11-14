# Built-in DS BIOS

`freebios7.bin` (ARM7, 16 KB) and `freebios9.bin` (ARM9, 4 KB) are the DS BIOS replacement by Gilead Kutnick, written for DraStic and BSD-2 licensed (see `LICENSE`). They boot games directly from their header. They cannot start the firmware menu.

Both images are the byte arrays in melonDS 1.1's `src/FreeBIOS.cpp`, written out unchanged. To extract them again:

```sh
git clone https://github.com/melonDS-emu/melonDS && cd melonDS && git checkout 1.1
python3 - <<'EOF'
import re
src = open("src/FreeBIOS.cpp").read()
for name, out in [("bios_arm7_bin", "freebios7.bin"), ("bios_arm9_bin", "freebios9.bin")]:
    body = re.search(name + r"\s*=\s*\{(.*?)\};", src, re.S).group(1)
    open(out, "wb").write(bytes(int(x, 16) for x in re.findall(r"0x([0-9a-fA-F]{2})", body)))
EOF
sha256sum freebios7.bin freebios9.bin
```

SHA-256:

- `freebios7.bin`: `80ae613b1cfd4cd56dd49f9e985b9788563388facaf0d0f5f78ea2b185f3f7c5`
- `freebios9.bin`: `39b4a574f4a24465fa4c7c9d2d4c847fdd46133b54528a22a7e198fe3a37e281`
