#include <mgba/core/blip_buf.h>
#include <mgba/core/core.h>
#include <mgba/core/log.h>
#include <mgba/core/timing.h>
#include <mgba-util/vfs.h>

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>

struct CitrineMgba {
	struct mCore* core;
	color_t* pixels;
	unsigned width;
	unsigned height;
	uint64_t cycles;
	uint32_t last_time;
};

static void citrine_mgba_log(struct mLogger* logger, int category, enum mLogLevel level, const char* format, va_list args) {
	(void) logger;
	if (!(level & (mLOG_FATAL | mLOG_ERROR))) {
		return;
	}
	fprintf(stderr, "mgba %s: ", mLogCategoryName(category));
	vfprintf(stderr, format, args);
	fputc('\n', stderr);
}

static struct mLogger citrine_mgba_logger = { .log = citrine_mgba_log, .filter = NULL };

static void citrine_mgba_drain_audio(struct mCore* core) {
	for (int channel = 0; channel < 2; ++channel) {
		struct blip_t* buffer = core->getAudioChannel(core, channel);
		if (buffer && blip_samples_avail(buffer) >= 1024) {
			blip_clear(buffer);
		}
	}
}

struct CitrineMgba* citrine_mgba_new(const void* rom, size_t rom_size, const void* bios, size_t bios_size) {
	mLogSetDefaultLogger(&citrine_mgba_logger);
	struct VFile* rom_file = VFileMemChunk(rom, rom_size);
	if (!rom_file) {
		return NULL;
	}
	struct mCore* core = mCoreFindVF(rom_file);
	if (!core) {
		rom_file->close(rom_file);
		return NULL;
	}
	struct CitrineMgba* emu = calloc(1, sizeof(*emu));
	if (!emu || !core->init(core)) {
		free(emu);
		rom_file->close(rom_file);
		return NULL;
	}
	emu->core = core;
	mCoreInitConfig(core, NULL);
	core->opts.useBios = bios != NULL;
	core->opts.skipBios = false;
	core->desiredVideoDimensions(core, &emu->width, &emu->height);
	emu->pixels = calloc((size_t) emu->width * emu->height, sizeof(color_t));
	core->setVideoBuffer(core, emu->pixels, emu->width);
	core->setAudioBufferSize(core, 2048);
	if (!core->loadROM(core, rom_file)) {
		mCoreConfigDeinit(&core->config);
		core->deinit(core);
		free(emu->pixels);
		free(emu);
		return NULL;
	}
	if (bios) {
		struct VFile* bios_file = VFileMemChunk(bios, bios_size);
		if (!bios_file || !core->loadBIOS(core, bios_file, 0)) {
			if (bios_file) {
				bios_file->close(bios_file);
			}
			mCoreConfigDeinit(&core->config);
			core->deinit(core);
			free(emu->pixels);
			free(emu);
			return NULL;
		}
	}
	core->reset(core);
	emu->last_time = (uint32_t) mTimingCurrentTime(core->timing);
	return emu;
}

void citrine_mgba_free(struct CitrineMgba* emu) {
	mCoreConfigDeinit(&emu->core->config);
	emu->core->deinit(emu->core);
	free(emu->pixels);
	free(emu);
}

unsigned citrine_mgba_width(const struct CitrineMgba* emu) {
	return emu->width;
}

unsigned citrine_mgba_height(const struct CitrineMgba* emu) {
	return emu->height;
}

const uint32_t* citrine_mgba_pixels(const struct CitrineMgba* emu) {
	return (const uint32_t*) emu->pixels;
}

void citrine_mgba_set_keys(struct CitrineMgba* emu, uint32_t keys) {
	emu->core->setKeys(emu->core, keys);
}

uint32_t citrine_mgba_run_loop(struct CitrineMgba* emu) {
	emu->core->runLoop(emu->core);
	uint32_t now = (uint32_t) mTimingCurrentTime(emu->core->timing);
	emu->cycles += (uint32_t) (now - emu->last_time);
	emu->last_time = now;
	citrine_mgba_drain_audio(emu->core);
	return emu->core->frameCounter(emu->core);
}

uint32_t citrine_mgba_frame_counter(const struct CitrineMgba* emu) {
	return emu->core->frameCounter(emu->core);
}

uint64_t citrine_mgba_cycles(const struct CitrineMgba* emu) {
	return emu->cycles;
}
