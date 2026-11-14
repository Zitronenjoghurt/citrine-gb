#include <algorithm>
#include <condition_variable>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <memory>
#include <mutex>
#include <thread>
#include <unordered_map>

#include <array>
#include <atomic>
#include <functional>
#include <list>
#include <map>
#include <optional>
#include <string>
#include <variant>
#include <vector>
#define private public
#include "GPU3D_Soft.h"
#undef private
#include "GPU3D.h"
#include "NDS.h"
#include "NDSCart.h"
#include "Platform.h"
#include "SPI_Firmware.h"

namespace melonDS::Platform
{

void SignalStop(StopReason, void*)
{
}

std::string GetLocalFilePath(const std::string& filename)
{
    return filename;
}

static std::string ModeString(FileMode mode, bool exists)
{
    std::string text;
    if (mode & FileMode::Append)
        text = (mode & FileMode::Read) ? "a+" : "a";
    else if (!(mode & FileMode::Write))
        text = "r";
    else if ((mode & FileMode::Preserve) && exists)
        text = "r+";
    else
        text = (mode & FileMode::Read) ? "w+" : "w";
    if (!(mode & FileMode::Text))
        text += "b";
    return text;
}

FileHandle* OpenFile(const std::string& path, FileMode mode)
{
    bool exists = FileExists(path);
    if (!exists && ((mode & FileMode::NoCreate) || !(mode & FileMode::Write)))
        return nullptr;
    return reinterpret_cast<FileHandle*>(std::fopen(path.c_str(), ModeString(mode, exists).c_str()));
}

FileHandle* OpenLocalFile(const std::string& path, FileMode mode)
{
    return OpenFile(GetLocalFilePath(path), mode);
}

bool FileExists(const std::string& name)
{
    FILE* file = std::fopen(name.c_str(), "rb");
    if (!file)
        return false;
    std::fclose(file);
    return true;
}

bool LocalFileExists(const std::string& name)
{
    return FileExists(GetLocalFilePath(name));
}

bool CheckFileWritable(const std::string& filepath)
{
    FILE* file = std::fopen(filepath.c_str(), "ab");
    if (!file)
        return false;
    std::fclose(file);
    return true;
}

bool CheckLocalFileWritable(const std::string& filepath)
{
    return CheckFileWritable(GetLocalFilePath(filepath));
}

static FILE* Raw(FileHandle* file)
{
    return reinterpret_cast<FILE*>(file);
}

bool CloseFile(FileHandle* file)
{
    return std::fclose(Raw(file)) == 0;
}

bool IsEndOfFile(FileHandle* file)
{
    return std::feof(Raw(file)) != 0;
}

bool FileReadLine(char* str, int count, FileHandle* file)
{
    return std::fgets(str, count, Raw(file)) != nullptr;
}

u64 FilePosition(FileHandle* file)
{
    return std::ftell(Raw(file));
}

bool FileSeek(FileHandle* file, s64 offset, FileSeekOrigin origin)
{
    int whence = SEEK_SET;
    if (origin == FileSeekOrigin::Current)
        whence = SEEK_CUR;
    else if (origin == FileSeekOrigin::End)
        whence = SEEK_END;
    return std::fseek(Raw(file), offset, whence) == 0;
}

void FileRewind(FileHandle* file)
{
    std::rewind(Raw(file));
}

u64 FileRead(void* data, u64 size, u64 count, FileHandle* file)
{
    return std::fread(data, size, count, Raw(file));
}

bool FileFlush(FileHandle* file)
{
    return std::fflush(Raw(file)) == 0;
}

u64 FileWrite(const void* data, u64 size, u64 count, FileHandle* file)
{
    return std::fwrite(data, size, count, Raw(file));
}

u64 FileWriteFormatted(FileHandle* file, const char* fmt, ...)
{
    va_list args;
    va_start(args, fmt);
    int written = std::vfprintf(Raw(file), fmt, args);
    va_end(args);
    return written < 0 ? 0 : written;
}

u64 FileLength(FileHandle* file)
{
    FILE* raw = Raw(file);
    long position = std::ftell(raw);
    std::fseek(raw, 0, SEEK_END);
    long length = std::ftell(raw);
    std::fseek(raw, position, SEEK_SET);
    return length;
}

void Log(LogLevel level, const char* fmt, ...)
{
    if (level != LogLevel::Error)
        return;
    va_list args;
    va_start(args, fmt);
    std::vfprintf(stderr, fmt, args);
    va_end(args);
}

struct Thread
{
    std::thread inner;
};

Thread* Thread_Create(std::function<void()> func)
{
    return new Thread{std::thread(std::move(func))};
}

void Thread_Free(Thread* thread)
{
    if (thread->inner.joinable())
        thread->inner.detach();
    delete thread;
}

void Thread_Wait(Thread* thread)
{
    if (thread->inner.joinable())
        thread->inner.join();
}

struct Semaphore
{
    std::mutex lock;
    std::condition_variable ready;
    int count = 0;
};

Semaphore* Semaphore_Create()
{
    return new Semaphore;
}

void Semaphore_Free(Semaphore* sema)
{
    delete sema;
}

void Semaphore_Reset(Semaphore* sema)
{
    std::lock_guard guard(sema->lock);
    sema->count = 0;
}

void Semaphore_Wait(Semaphore* sema)
{
    std::unique_lock guard(sema->lock);
    sema->ready.wait(guard, [sema] { return sema->count > 0; });
    sema->count--;
}

bool Semaphore_TryWait(Semaphore* sema, int timeout_ms)
{
    std::unique_lock guard(sema->lock);
    if (!sema->ready.wait_for(guard, std::chrono::milliseconds(timeout_ms), [sema] { return sema->count > 0; }))
        return false;
    sema->count--;
    return true;
}

void Semaphore_Post(Semaphore* sema, int count)
{
    {
        std::lock_guard guard(sema->lock);
        sema->count += count;
    }
    sema->ready.notify_all();
}

struct Mutex
{
    std::mutex inner;
};

Mutex* Mutex_Create()
{
    return new Mutex;
}

void Mutex_Free(Mutex* mutex)
{
    delete mutex;
}

void Mutex_Lock(Mutex* mutex)
{
    mutex->inner.lock();
}

void Mutex_Unlock(Mutex* mutex)
{
    mutex->inner.unlock();
}

bool Mutex_TryLock(Mutex* mutex)
{
    return mutex->inner.try_lock();
}

void Sleep(u64 usecs)
{
    std::this_thread::sleep_for(std::chrono::microseconds(usecs));
}

void WriteNDSSave(const u8*, u32, u32, u32, void*)
{
}

void WriteGBASave(const u8*, u32, u32, u32, void*)
{
}

void WriteFirmware(const Firmware&, u32, u32, void*)
{
}

void WriteDateTime(int, int, int, int, int, int, void*)
{
}

void MP_Begin(void*)
{
}

void MP_End(void*)
{
}

int MP_SendPacket(u8*, int, u64, void*)
{
    return 0;
}

int MP_RecvPacket(u8*, u64*, void*)
{
    return 0;
}

int MP_SendCmd(u8*, int, u64, void*)
{
    return 0;
}

int MP_SendReply(u8*, int, u64, u16, void*)
{
    return 0;
}

int MP_SendAck(u8*, int, u64, void*)
{
    return 0;
}

int MP_RecvHostPacket(u8*, u64*, void*)
{
    return 0;
}

u16 MP_RecvReplies(u8*, u64, u16, void*)
{
    return 0;
}

int Net_SendPacket(u8*, int, void*)
{
    return 0;
}

int Net_RecvPacket(u8*, void*)
{
    return 0;
}

void Camera_Start(int, void*)
{
}

void Camera_Stop(int, void*)
{
}

void Camera_CaptureFrame(int, u32*, int, int, bool, void*)
{
}

void Mic_Start(void*)
{
}

void Mic_Stop(void*)
{
}

int Mic_ReadInput(s16*, int, void*)
{
    return 0;
}

AACDecoder* AAC_Init()
{
    return nullptr;
}

void AAC_DeInit(AACDecoder*)
{
}

bool AAC_Configure(AACDecoder*, int, int)
{
    return false;
}

bool AAC_DecodeFrame(AACDecoder*, const void*, int, void*, int)
{
    return false;
}

bool Addon_KeyDown(KeyType, void*)
{
    return false;
}

void Addon_RumbleStart(u32, void*)
{
}

void Addon_RumbleStop(void*)
{
}

float Addon_MotionQuery(MotionQueryType, void*)
{
    return 0.0f;
}

DynamicLibrary* DynamicLibrary_Load(const char*)
{
    return nullptr;
}

void DynamicLibrary_Unload(DynamicLibrary*)
{
}

void* DynamicLibrary_LoadFunction(DynamicLibrary*, const char*)
{
    return nullptr;
}

}

using namespace melonDS;

struct CitrineVertex
{
    int32_t position[4];
    int32_t screen[2];
    int32_t hires[2];
    int32_t color[3];
    int32_t texcoord[2];
    uint32_t clipped;
};

struct CitrinePolygon
{
    uint32_t vertices[10];
    uint32_t count;
    int32_t z[10];
    int32_t w[10];
    uint32_t attributes;
    uint32_t texture;
    uint32_t palette;
    uint32_t w_buffer;
    uint32_t degenerate;
    uint32_t front;
    uint32_t translucent;
    uint32_t top;
    uint32_t bottom;
};

struct CitrineScene3d
{
    uint32_t disp3dcnt;
    uint32_t alpha_reference;
    uint32_t clear_color;
    uint32_t clear_depth;
    uint32_t fog_color;
    uint32_t fog_offset;
    uint32_t toon[32];
    uint32_t edge[8];
    uint32_t fog_density[32];
    uint32_t polygon_count;
    uint32_t vertex_count;
    CitrinePolygon polygons[2048];
    CitrineVertex vertices[6144 * 2];
    uint8_t texture[512 * 1024];
    uint8_t palette[128 * 1024];
    uint32_t layer[256 * 192];
    uint32_t plain[256 * 192];
    uint32_t depth[256 * 192];
    uint32_t attributes[256 * 192];
    uint32_t under_color[256 * 192];
    uint32_t under_depth[256 * 192];
    uint32_t under_attributes[256 * 192];
};

struct CitrineMelonds
{
    std::unique_ptr<NDS> nds;
    u32 frames = 0;
};

extern "C" {

static CitrineMelonds* create(const void* rom, size_t rom_size, const void* bios9, size_t bios9_size,
                              const void* bios7, size_t bios7_size, unsigned sample_rate, bool jit)
{
    if (bios9_size != ARM9BIOSSize || bios7_size != ARM7BIOSSize || rom_size > UINT32_MAX)
        return nullptr;
    auto cart = NDSCart::ParseROM(static_cast<const u8*>(rom), static_cast<u32>(rom_size), nullptr, std::nullopt);
    if (!cart)
        return nullptr;
    NDSArgs args;
    std::memcpy(args.ARM9BIOS->data(), bios9, ARM9BIOSSize);
    std::memcpy(args.ARM7BIOS->data(), bios7, ARM7BIOSSize);
    if (!jit)
        args.JIT = std::nullopt;
    args.OutputSampleRate = sample_rate;
    auto emu = new CitrineMelonds;
    emu->nds = std::make_unique<NDS>(std::move(args), emu);
    emu->nds->SetNDSCart(std::move(cart));
    emu->nds->Reset();
    emu->nds->SetupDirectBoot("game.nds");
    emu->nds->ARM9.CP15Write(0x100, 0x00052078);
    emu->nds->Start();
    return emu;
}

CitrineMelonds* citrine_melonds_new(const void* rom, size_t rom_size, const void* bios9, size_t bios9_size,
                                    const void* bios7, size_t bios7_size, unsigned sample_rate)
{
    return create(rom, rom_size, bios9, bios9_size, bios7, bios7_size, sample_rate, false);
}

#ifdef JIT_ENABLED
CitrineMelonds* citrine_melonds_new_jit(const void* rom, size_t rom_size, const void* bios9, size_t bios9_size,
                                        const void* bios7, size_t bios7_size, unsigned sample_rate)
{
    return create(rom, rom_size, bios9, bios9_size, bios7, bios7_size, sample_rate, true);
}
#endif

void citrine_melonds_free(CitrineMelonds* emu)
{
    delete emu;
}

void citrine_melonds_set_save(CitrineMelonds* emu, const void* data, size_t size)
{
    emu->nds->SetNDSSave(static_cast<const u8*>(data), static_cast<u32>(size));
}

void citrine_melonds_set_keys(CitrineMelonds* emu, uint32_t keys)
{
    emu->nds->SetKeyMask(~keys & 0xFFF);
}

void citrine_melonds_touch(CitrineMelonds* emu, unsigned x, unsigned y, unsigned down)
{
    if (down)
        emu->nds->TouchScreen(x, y);
    else
        emu->nds->ReleaseScreen();
}

uint32_t citrine_melonds_run_frame(CitrineMelonds* emu)
{
    emu->nds->RunFrame();
    return ++emu->frames;
}

const uint32_t* citrine_melonds_screen(const CitrineMelonds* emu, unsigned index)
{
    const GPU& gpu = emu->nds->GPU;
    return gpu.Framebuffer[gpu.FrontBuffer][index & 1].get();
}

uint64_t citrine_melonds_cycles(const CitrineMelonds* emu)
{
    return emu->nds->ARM7Timestamp;
}

size_t citrine_melonds_read_audio(CitrineMelonds* emu, int16_t* out, size_t max_frames)
{
    return static_cast<size_t>(emu->nds->SPU.ReadOutput(out, static_cast<int>(max_frames)));
}

void citrine_melonds_scene3d(CitrineMelonds* emu, CitrineScene3d* out)
{
    GPU& gpu = emu->nds->GPU;
    GPU3D& gpu3d = gpu.GPU3D;
    u32 dispcnt = gpu3d.RenderDispCnt;
    gpu3d.RenderDispCnt = dispcnt & ~((1u << 4) | (1u << 5) | (1u << 7));
    gpu3d.RenderFrameIdentical = false;
    gpu3d.GetCurrentRenderer().RenderFrame(gpu);
    for (int y = 0; y < 192; y++)
        std::memcpy(&out->plain[y * 256], gpu3d.GetCurrentRenderer().GetLine(y), 256 * 4);
    gpu3d.RenderDispCnt = dispcnt;
    gpu3d.RenderFrameIdentical = false;
    gpu3d.GetCurrentRenderer().RenderFrame(gpu);

    out->disp3dcnt = gpu3d.RenderDispCnt;
    out->alpha_reference = gpu3d.RenderAlphaRef;
    out->clear_color = gpu3d.RenderClearAttr1;
    out->clear_depth = gpu3d.RenderClearAttr2;
    out->fog_color = gpu3d.RenderFogColor;
    out->fog_offset = gpu3d.RenderFogOffset / 0x200;
    for (int i = 0; i < 32; i++)
    {
        out->toon[i] = gpu3d.RenderToonTable[i];
        out->fog_density[i] = gpu3d.RenderFogDensityTable[i + 1];
    }
    for (int i = 0; i < 8; i++)
        out->edge[i] = gpu3d.RenderEdgeTable[i];

    std::unordered_map<const Vertex*, uint32_t> seen;
    uint32_t vertex_count = 0;
    uint32_t polygon_count = std::min<uint32_t>(gpu3d.RenderNumPolygons, 2048);
    for (uint32_t i = 0; i < polygon_count; i++)
    {
        const Polygon* poly = gpu3d.RenderPolygonRAM[i];
        CitrinePolygon& dst = out->polygons[i];
        dst.count = std::min<uint32_t>(poly->NumVertices, 10);
        for (uint32_t v = 0; v < dst.count; v++)
        {
            const Vertex* vtx = poly->Vertices[v];
            auto found = seen.find(vtx);
            uint32_t index = found == seen.end() ? vertex_count : found->second;
            if (found == seen.end() && vertex_count < 6144 * 2)
            {
                seen.emplace(vtx, vertex_count++);
                CitrineVertex& cv = out->vertices[index];
                for (int k = 0; k < 4; k++)
                    cv.position[k] = vtx->Position[k];
                for (int k = 0; k < 2; k++)
                {
                    cv.screen[k] = vtx->FinalPosition[k];
                    cv.hires[k] = vtx->HiresPosition[k];
                    cv.texcoord[k] = vtx->TexCoords[k];
                }
                for (int k = 0; k < 3; k++)
                    cv.color[k] = vtx->FinalColor[k];
                cv.clipped = vtx->Clipped;
            }
            dst.vertices[v] = index;
            dst.z[v] = poly->FinalZ[v];
            dst.w[v] = poly->FinalW[v];
        }
        dst.attributes = poly->Attr;
        dst.texture = poly->TexParam;
        dst.palette = poly->TexPalette;
        dst.w_buffer = poly->WBuffer;
        dst.degenerate = poly->Degenerate;
        dst.front = poly->FacingView;
        dst.translucent = poly->Translucent;
        dst.top = poly->VTop;
        dst.bottom = poly->VBottom;
    }
    out->polygon_count = polygon_count;
    out->vertex_count = vertex_count;

    std::memcpy(out->texture, gpu.VRAMFlat_Texture, sizeof(out->texture));
    std::memcpy(out->palette, gpu.VRAMFlat_TexPal, sizeof(out->palette));
    for (int y = 0; y < 192; y++)
        std::memcpy(&out->layer[y * 256], gpu3d.GetCurrentRenderer().GetLine(y), 256 * 4);
    auto* soft = dynamic_cast<SoftRenderer*>(&gpu3d.GetCurrentRenderer());
    if (soft)
    {
        for (int y = 0; y < 192; y++)
        {
            int row = SoftRenderer::FirstPixelOffset + y * SoftRenderer::ScanlineWidth;
            int under = row + SoftRenderer::BufferSize;
            std::memcpy(&out->depth[y * 256], &soft->DepthBuffer[row], 256 * 4);
            std::memcpy(&out->attributes[y * 256], &soft->AttrBuffer[row], 256 * 4);
            std::memcpy(&out->under_color[y * 256], &soft->ColorBuffer[under], 256 * 4);
            std::memcpy(&out->under_depth[y * 256], &soft->DepthBuffer[under], 256 * 4);
            std::memcpy(&out->under_attributes[y * 256], &soft->AttrBuffer[under], 256 * 4);
        }
    }
}

}
