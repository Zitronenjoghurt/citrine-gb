#include <stdio.h>

#include <nds.h>

static void print_name(const char *label, const u16 *name, int length)
{
    printf("%s ", label);
    for (int i = 0; i < length && i < 10; i++)
        printf("%c", (char)name[i]);
    printf("\n");
}

int main(int argc, char **argv)
{
    consoleDemoInit();

    print_name("NICKNAME", PersonalData->name, PersonalData->nameLen);
    printf("COLOR %d\n", PersonalData->theme);
    printf("BIRTHDAY %d/%d\n", PersonalData->birthMonth, PersonalData->birthDay);
    printf("LANGUAGE %d\n", PersonalData->language);

    u16 offset = 0;
    u8 block[0x74];
    readFirmware(0x20, &offset, sizeof(offset));
    readFirmware((u32)offset * 8 + 0x100, block, sizeof(block));
    print_name("FLASH", (const u16 *)(block + 6), block[0x1A]);

    while (1)
        swiWaitForVBlank();
}
