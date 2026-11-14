# DS Download Play

How a DS with no game card receives a game from a host over local wireless, and how citrine-gb does it with no BIOS or firmware dump.

## Why this exists

On real hardware the player without the card runs the firmware's Download Play program. It listens for the host's beacons, joins, receives a signed multiboot image, checks the RSA signature and boots it. That program lives in the firmware, which we can't ship. So the guest's console runs no firmware code at all. `nds/download.rs` is a receiver written in Rust that speaks the same frames on our air model, and `nds/machine/download.rs` boots the image it receives with FreeBIOS and skips the signature check. The child game then talks to the host over the same air as any local wireless game.

## Sources

Tags used below:

| Tag | Source |
| --- | --- |
| GB | GBATEK, DS Wifi Nintendo Beacons, http://problemkaputt.de/gbatek-ds-wifi-nintendo-beacons.htm |
| GD | GBATEK, DS Wifi Nintendo DS Download Play, http://problemkaputt.de/gbatek-ds-wifi-nintendo-ds-download-play.htm |
| GH | GBATEK, DS Wifi Hardware Headers, https://problemkaputt.de/gbatek-ds-wifi-hardware-headers.htm |
| GM | GBATEK, IEEE802.11 Management Frames, http://problemkaputt.de/gbatek-ds-wifi-ieee802-11-managment-frames-type-0.htm |
| GMM | GBATEK, DS Wifi Multiplay Master, http://problemkaputt.de/gbatek-ds-wifi-multiplay-master.htm |
| GR | GBATEK, BIOS RAM Usage, http://problemkaputt.de/gbatek-bios-ram-usage.htm |
| RI | Retro Isle, DS wireless notes, http://retroisle.com/others/nintendods/Technical/Firmware/wireless.php |
| NES | nesdev forum thread "NDS Wifi specs", https://forums.nesdev.org/viewtopic.php?t=21141 |
| WMB | yellows8/wmb-asm, `wmb_asm_sdk.h`, `wmb.c`, `utils.c` on GitHub |
| DLP | lesserkuma/DSDLP_Assembler, `in_pcap.py` on GitHub |
| PD | pret/pokediamond, `arm9/lib/NitroSDK/include/MB_mb.h` |
| MK | our own air log of Mario Kart DS (EUR) hosting Download Play against the receiver, `.dlp` lab runs on 2026-11-08 |

All web pages were read through the web-reader agent on 2026-11-08.

## Beacons

The host sends a beacon about five times a second (GD). There is no SSID element; the game is announced in a Nintendo vendor element (tag 0xDD, OUI `00 09 BF 00`), type 0x0B for multiboot (GB). Offsets below are relative to the OUI.

| Off | Size | Field |
| --- | --- | --- |
| 0x0C | 4 | Game id (GGID) |
| 0x10 | 2 | Stream code (TGID), random per host session |
| 0x13 | 1 | Beacon type, 0x0B for multiboot |
| 0x14 | 2 | CMD size in bytes, for example 0x01FE (packet size is this minus 6) |
| 0x1D | 1 | Session number, goes up when the host restarts the upload |
| 0x1F | 1 | Fragment number |
| 0x20 | 2 | Checksum of the halfwords from 0x22 to 0x87: `~((sum >> 16) + (sum & 0xFFFF))` |
| 0x23 | 1 | Highest fragment number, 9 |
| 0x24 | 2 | Data size of this fragment |
| 0x26 | 0x62 | Data |

Fragments 0 to 8 hold the 0x358-byte advert; fragment 9 lists the connected players (GB, WMB, DLP). The advert holds the icon palette (0x000), icon tiles (0x020), the host's name length and UCS-2 name (0x221, 0x222), the player limit (0x236), the title (0x238, 48 units) and the description (0x298, 96 units). `download/advert.rs` assembles the fragments per host and keeps the offer while beacons keep coming, and drops it after 3 s of silence.

## Joining

The client sends Auth (algorithm 0, sequence 1), the host answers Auth sequence 2 with status 0, the client sends an Assoc request whose SSID element is 0x20 bytes of GGID, TGID and zeros, and the host answers with status 0 and an AID with bits 14 and 15 set (GB, GM, NES). Our air model has no 802.11 ACKs, so the host's chip neither waits for one nor resends. The receiver sends each request 200 µs after a beacon from that host and repeats it every 400 ms until answered, giving up after 20 tries.

## Transfer

The host polls with MP CMD frames (FC 0x0228, address 1 `03 09 BF 00 00 00`). The body is the reply time, the client mask, a size byte in halfwords, a flags byte, a command byte and its data (GD).

| Cmd | Name | Data |
| --- | --- | --- |
| 1 | Name request | 5 zero bytes |
| 3 | RSA | the 0xE4-byte RSA frame, then 5 zero bytes (size byte 0x75) |
| 4 | Data | 2 zero bytes, a u16 packet number, the packet, 1 zero byte |
| 5 | Final | 5 bytes |

Each client in the mask answers with an MP reply (FC 0x0118, address 3 `03 09 BF 00 00 10`, short preamble at 2 Mbit/s) in its own slot, `16 + (10 + reply time) × players below it` µs after the CMD ends (GMM). The 10-byte reply is `04 81`, a type and 7 payload bytes (GD, RI): 0x07 a name snippet, 0x08 RSA received, 0x09 the next missing packet and the count received, 0x0A all received, 0x0B final.

The reply follows the client's state, not the command it answers (MK). Before the RSA frame the client answers a name request with the next name snippet and anything else with type 0x00. Once it holds the RSA frame it answers every command with 0x08 until the first packet arrives, then with 0x09 until it has every packet, then with 0x0A, and it answers Final with 0x0B. Mario Kart DS sends no data to a client that answers 0x00 or 0x09 before its first packet: it keeps sending Dummy commands and its screen stays on "PLEASE WAIT...".

The RSA frame names the header, ARM9 and ARM7 sizes at 0x14, 0x24 and 0x34. Packets are numbered from 0 over the header, then the ARM9 binary, then the ARM7 binary, and each section starts a new packet (GD, RI, DLP). Nothing but the 802.11 FCS protects a packet (GD), so the receiver checks the header CRC16 at 0x15E, the binary sizes against the header, and that the binaries fit and don't overlap (`download/image.rs`). A child that fails a check isn't booted: after its Deauth the receiver reports the download as stopped, and pressing A sends it back to listening, so it joins again if the host still offers the game.

After Final the client sends a Deauth with reason 3 and boots (GD). The receiver sends its Deauth after the host's MP ACK, because the chip drops other frames in the reply window.

## Boot

Download Play leaves a boot indicator of 2 at 0x027FFC40 and the host's BSS description at 0x027FFC42 to 0x027FFC7F (GD, GR; PD names it `MBParentBssDesc`: length 0x60, RSSI, BSSID, SSID length and SSID, capability 0x0021, rate sets 0x0003, beacon period, DTIM period 2 and channel). The child reads it to find the host again, and joins it with a fresh Auth and Assoc on its own wifi chip. `Nds::boot_download` builds a ROM image with the binaries at the header's own offsets, direct-boots it with FreeBIOS, writes those two blocks, and keeps the console on the air with its air clock running on.

The real firmware checks the RSA signature and restarts before the child runs, and the host restarts its own wireless in that time. Our boot skips the check, so the receiver holds the image for `BOOT_DELAY_US` (3 s) after its Deauth. With MK, a child booted at once sent its first Auth 1 s after the Deauth and got no answer; booted 1 s later it got an Auth answer but no Assoc answer. Either way its retry joined, and then MK's link broke down and the host dropped the child about 40 s later. With 2 s and 3 s the child joined on its first try and both consoles reached MK's game select screen.

The child also keeps the waiting console's scan position, its line and dot. A real DS doesn't power-cycle to start the binary, so its LCD keeps counting lines through the hand-off. This matters for the linked step: Play together and the lab runner step linked consoles one frame at a time, and a frame ends when line 192 starts. A child started at line 0 ends its first frame 71 lines (about 4.5 ms) early and keeps that offset. Every MP exchange that falls between the two frame ends then misses its reply window, because the other console has already stopped for that frame. In the Mario Kart DS run this cost 30 to 50% CMD retries during MK's own second-stage download and course download. One message (`07 94 0a 02`) failed all 13 retries every time, and both consoles stayed on a black screen from about frame 10500.

## What the Mario Kart DS run showed

- MK hosts Download Play from Multiplayer, CREATE GROUP, then SIMPLE ("Compete with players who don't have Mario Kart DS"). NORMAL sends MK's own multi-card beacons (type 0x01), which the receiver ignores.
- The host sends six name requests first, then the RSA frame twice (size byte 0x75). No empty RSA pings came.
- No data comes until the host's player touches CUT OFF, and then only to a client that keeps answering 0x08.
- CMD size 0x1FE, so 504-byte packets: 2211 packets, about 1.1 MB, in about 640 frames. Packets 1 to 3 came before packet 0 (the 0x160-byte header), then the rest in order.
- After the child boots it joins the host again with a fresh Auth and Assoc on its own chip, and MK's child then downloads more over MK's own protocol for about 2300 frames ("Downloading... Please wait.").
- MK's ARM7 wireless library writes VCOUNT to line its frames up with the host's. Our LCD takes the write as the next line's number, as GBATEK says, and drops values past the last line. With that, the child's lines and the host's agree within one line.

## The data-sharing freeze

After the course download, MK starts its race-time data sharing: 6 MPs a frame from the host, each child reply carrying a sequence number. In the lab run that sequence stops at 0x84c on one console and 0x84e on the other, at frames 8869 to 8871, and the race never starts.

- The host marks the last CMD of each frame's burst with the VSYNC bit (bit 15 of the wireless header). The child builds its next reply at its own MP alarm, line 240, only if that CMD came in before it. From frame 8865 the VSYNC CMD ends late: at frame 8864 it ends at child line 249, 9 lines (about 570 us) past the alarm. The child then drops every CMD until its next alarm, and data sharing never recovers. A lab build that clears bit 15 on the child's received CMDs runs data sharing on to sequence 0x8c6 at frame 9297, so the late VSYNC CMD is the whole freeze.
- A burst of 6 MPs takes 16.4 ms at frame 8864. Each MP round (CMD start to MP end) takes 856 us for a 40-byte CMD and up to 2488 us for a 448-byte one, and each gap from an MP end to the next CMD takes 820 to 1440 us. The burst needs about 120 us less in each of its 5 gaps to fit.
- Not the CPUs. A host ARM9 trace shows it reads each ARM7 hand-off 3 to 17 us after the send. A host ARM7 trace with the cost of every instruction shows about 4800 instructions (433 us) of WM and wifi code after the last hand-off of a gap, at costs that match GBATEK or come in under it: the BIOS wait loop takes 3 cycles a pass, where GBATEK gives 4. ARM9 Load Timing leaves all 4300 data-sharing lines of the run unchanged.
- Not the line alignment. The child's lines and the host's agree within one line.
- The cause was the chip's duplicate check. A child that can't answer a CMD drops it and writes 0xFFFF to wifi RAM 0x1F7E (NitroSDK's `MREG_MP_LAST_SEQCTRL`, written in `WlIntrRxEnd` and `InitRxCtrl`), where the chip keeps the sequence control of the last CMD it took. The host's resend keeps the sequence number, so the reset is what lets the resend through. Ours kept that record inside the emulator and ignored the write: the resend counted as a duplicate and never reached the game, and from frame 8865 every VSYNC CMD needed resends until after line 240. The chip now keeps the record at 0x1F7E. In the lab run MK writes 0xFFFF there 2144 times between frames 8865 and 9311, and the race starts by frame 10000.

## What is still open

- The order of the data reply's two fields. GD gives next missing then count, RI the other way. MK sent its packets in order whatever the reply said, so the run doesn't settle it.
