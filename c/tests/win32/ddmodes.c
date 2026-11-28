/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt), programme de tests/hx_ddmodes.rs
   DirectDraw de HX (DDDRAW, fork de Bare86) : EnumDisplayModes et GetDisplayMode décrivent les modes comme Windows :
   les modes 8 bits ont DDPF_PALETTEINDEXED8, aucun mode n'a 15 bits par pixel (un mode 5-5-5 est annoncé en 16 bits avec
   ses masques ; OpenPTC refuse sinon « unsupported bits per pixel »). Résultats dans DDMODES.TXT. */
#include <windows.h>
#include <ddraw.h>
#include <stdio.h>

static int n, n8, n8pal, n15, n555, nbad;

static HRESULT WINAPI cb(LPDDSURFACEDESC d, LPVOID ctx)
{
    DDPIXELFORMAT *p = &d->ddpfPixelFormat;
    n++;
    if (p->dwRGBBitCount == 8) { n8++; if (p->dwFlags & DDPF_PALETTEINDEXED8) n8pal++; }
    else if (p->dwRGBBitCount == 15) n15++;
    else if (p->dwRGBBitCount == 16 && p->dwRBitMask == 0x7C00 && p->dwGBitMask == 0x03E0 && p->dwBBitMask == 0x001F) n555++;
    else if (p->dwRGBBitCount != 16 && p->dwRGBBitCount != 24 && p->dwRGBBitCount != 32) nbad++;
    return DDENUMRET_OK;
}

int main(void)
{
    HRESULT (WINAPI *create)(GUID *, LPDIRECTDRAW *, IUnknown *);
    HMODULE m = LoadLibraryA("ddraw.dll");
    LPDIRECTDRAW dd = NULL;
    FILE *f;
    if (m && (create = (void *)GetProcAddress(m, "DirectDrawCreate")) && create(NULL, &dd, NULL) == DD_OK)
        IDirectDraw_EnumDisplayModes(dd, 0, NULL, NULL, cb);
    f = fopen("DDMODES.TXT", "w");
    fprintf(f, "modes=%d pal8=%d bits15=%d rgb555=%d bad=%d\n", n > 10, n8 > 0 && n8pal == n8, n15, n555 > 0, nbad);
    fclose(f);
    return 0;
}
