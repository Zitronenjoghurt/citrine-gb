# DS test ROM

`blocksds-console-both-screens.nds` is BlocksDS's `examples/console/both_screens` from github.com/blocksds/sdk at fb7009f6e345e5521a6501cdf8590e9de3a08bf8, the same file as in the ledger's `nds-test-roms-1.zip` (SHA-256 dc7b923aefed358c0a39dcbd8c6aad2e00ff01b26dc549386b505db540ee7338). The example is CC0-1.0 (`cc0-1.0.txt`); the BlocksDS libraries it links are zlib (`zlib.txt`) and MIT (`mit.txt`).

`nickname.nds` is built here from `nickname/` with `nickname/build.sh` (BlocksDS in docker). It prints the firmware user settings it reads, from RAM and over the firmware chip. Its own code is part of this repository; the BlocksDS libraries it links are zlib (`zlib.txt`) and MIT (`mit.txt`).

`blocksds-lighting_cube.nds` is BlocksDS's `examples/graphics_3d/lighting_cube` from the same commit, built with BlocksDS in docker. The example is CC0-1.0 (`cc0-1.0.txt`); the libraries it links are zlib (`zlib.txt`) and MIT (`mit.txt`).

`airtalk.nds` is built here from `airtalk/` with `airtalk/build.sh` (BlocksDS in docker). Two DSes running it talk over local wireless: the one whose MAC ends in an even byte hosts and beacons, the other scans, connects and replies each frame, and both keep a report in main RAM that the netplay tests read. Its own code is part of this repository; the BlocksDS libraries it links, dswifi among them, are zlib (`zlib.txt`) and MIT (`mit.txt`).
