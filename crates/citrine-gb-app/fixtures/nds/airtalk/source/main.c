#include <stdio.h>
#include <string.h>

#include <nds.h>
#include <dswifi9.h>

#define GAME_ID 0x00404354

typedef struct {
    u32 frame;
    u16 keys;
    u8 touch_x;
    u8 touch_y;
} Packet;

typedef struct {
    char magic[8];
    u32 role;
    u32 linked;
    u32 sent;
    u32 heard;
    u32 last_frame;
    u32 last_keys;
    u32 checksum;
} Report;

volatile Report report = { .magic = "AIRTALK", .role = 0 };

static Packet outgoing;

static void mix(const Packet *packet)
{
    report.heard++;
    report.last_frame = packet->frame;
    report.last_keys = packet->keys;
    report.checksum = report.checksum * 31 + packet->frame * 7 + packet->keys
                      + (packet->touch_x << 16) + (packet->touch_y << 24);
}

static void from_client(Wifi_MPPacketType type, int aid, int base, int len)
{
    (void)aid;
    if (type != WIFI_MPTYPE_REPLY || len < (int)sizeof(Packet))
        return;
    Packet packet;
    Wifi_RxRawReadPacket(base, sizeof(packet), &packet);
    mix(&packet);
}

static void from_host(Wifi_MPPacketType type, int base, int len)
{
    if (type != WIFI_MPTYPE_CMD || len < (int)sizeof(Packet))
        return;
    Packet packet;
    Wifi_RxRawReadPacket(base, sizeof(packet), &packet);
    mix(&packet);
}

static void fill(u32 frame)
{
    touchPosition touch = { 0 };
    u16 keys = keysHeld();
    if (keys & KEY_TOUCH)
        touchRead(&touch);
    outgoing.frame = frame;
    outgoing.keys = keys;
    outgoing.touch_x = touch.px;
    outgoing.touch_y = touch.py;
}

static void wait_ready(void)
{
    while (!Wifi_LibraryModeReady())
        swiWaitForVBlank();
}

static void host(void)
{
    report.role = 1;
    Wifi_MultiplayerHostMode(1, sizeof(Packet), sizeof(Packet));
    wait_ready();
    Wifi_MultiplayerFromClientSetPacketHandler(from_client);
    Wifi_BeaconStart("citrine-airtalk", GAME_ID);
    for (u32 frame = 0;; frame++) {
        swiWaitForVBlank();
        scanKeys();
        fill(frame);
        report.linked = Wifi_MultiplayerGetNumClients();
        while (REG_VCOUNT != 96)
            ;
        if (report.linked > 0
            && Wifi_MultiplayerHostCmdTxFrame(&outgoing, sizeof(outgoing)) == 0)
            report.sent++;
        consoleClear();
        printf("HOST\nCLIENTS %lu\nSENT %lu\nHEARD %lu\nLAST %lu\n",
               report.linked, report.sent, report.heard, report.last_frame);
    }
}

static void client(void)
{
    report.role = 2;
    Wifi_MultiplayerClientMode(sizeof(Packet));
    wait_ready();
    Wifi_MultiplayerFromHostSetPacketHandler(from_host);
    Wifi_ScanMode();
    Wifi_AccessPoint ap;
    for (;;) {
        swiWaitForVBlank();
        consoleClear();
        printf("CLIENT\nLOOKING\n");
        int found = 0;
        int count = Wifi_GetNumAP();
        for (int i = 0; i < count; i++) {
            if (Wifi_GetAPData(i, &ap) != WIFI_RETURN_OK)
                continue;
            if ((ap.flags & WFLAG_APDATA_NINTENDO_TAG) && ap.nintendo.game_id == GAME_ID) {
                found = 1;
                break;
            }
        }
        if (found)
            break;
    }
    Wifi_ConnectOpenAP(&ap);
    for (u32 frame = 0;; frame++) {
        swiWaitForVBlank();
        scanKeys();
        fill(frame);
        report.linked = Wifi_AssocStatus() == ASSOCSTATUS_ASSOCIATED;
        if (report.linked
            && Wifi_MultiplayerClientReplyTxFrame(&outgoing, sizeof(outgoing)) == 0)
            report.sent++;
        consoleClear();
        printf("CLIENT\nLINKED %lu\nSENT %lu\nHEARD %lu\nLAST %lu\n",
               report.linked, report.sent, report.heard, report.last_frame);
    }
}

int main(int argc, char **argv)
{
    (void)argc;
    (void)argv;
    consoleDemoInit();
    if (!Wifi_InitDefault(WIFI_LOCAL_ONLY | INIT_ONLY | WIFI_DISABLE_LED)) {
        printf("NO WIFI\n");
        for (;;)
            swiWaitForVBlank();
    }
    u8 mac[6];
    Wifi_GetData(WIFIGETDATA_MACADDRESS, sizeof(mac), mac);
    if ((mac[5] & 1) == 0)
        host();
    else
        client();
    return 0;
}
