BLOCKSDS ?= /opt/blocksds/core

NAME := airtalk
GAME_TITLE := Air talk
GAME_SUBTITLE := Local wireless host and client
GAME_AUTHOR := Citrine

LIBS := -ldswifi9_noip -lnds9
LIBDIRS := $(BLOCKSDS)/libs/dswifi $(BLOCKSDS)/libs/libnds
ARM7ELF := $(BLOCKSDS)/sys/arm7/main_core/arm7_dswifi.elf

include $(BLOCKSDS)/sys/default_makefiles/rom_arm9/Makefile
