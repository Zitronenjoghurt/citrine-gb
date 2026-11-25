#include <stdio.h>

#include <nds.h>

#define RATE 16384
#define HALF 1024

typedef struct {
    char magic[8];
    u32 rate;
    u32 buffers;
    u32 samples;
    u32 level;
    u32 crossings;
    u32 quiet;
    u32 loud;
} Report;

volatile Report report = { .magic = "MICTEST", .rate = RATE };

static s16 buffer[HALF * 2] ALIGN(32);

static void heard(void *completed, int length)
{
    const s16 *samples = completed;
    int count = length / 2;
    DC_InvalidateRange(completed, length);

    s32 sum = 0;
    for (int i = 0; i < count; i++)
        sum += samples[i];
    s32 mean = sum / count;

    u32 level = 0;
    u32 crossings = 0;
    int above = samples[0] > mean;
    for (int i = 0; i < count; i++) {
        s32 delta = samples[i] - mean;
        level += delta < 0 ? -delta : delta;
        int now = samples[i] > mean;
        crossings += now != above;
        above = now;
    }
    level /= count;

    report.buffers++;
    report.samples = count;
    report.level = level;
    report.crossings = crossings;
    if (level < 64)
        report.quiet++;
    else
        report.loud++;
}

int main(int argc, char **argv)
{
    consoleDemoInit();
    soundEnable();
    soundMicRecord(buffer, sizeof(buffer), MicFormat_12Bit, RATE, heard);

    while (1) {
        swiWaitForVBlank();
        consoleClear();
        printf("MIC %s\n", report.level < 64 ? "QUIET" : "LOUD");
        printf("LEVEL %lu\n", report.level);
        printf("CROSSINGS %lu / %lu\n", report.crossings, report.samples);
        printf("BUFFERS %lu\n", report.buffers);
    }
}
