/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt), programme de tests/hx_textout.rs
   TextOutA de DGDI32 (fork de HX de Bare86) dans un bitmap mémoire de bas en haut (DIB section, hauteur positive) :
   le texte écrit en haut (y = 1) doit se trouver dans les lignes du haut de l'image, c'est-à-dire à la FIN des bits ;
   du texte hors du bitmap (coordonnées négatives, au-delà du bord) ne doit rien écrire ni planter. Résultats dans TEXTOUT.TXT. */
#include <windows.h>
#include <stdio.h>

#define W 64
#define H 64

static int count(unsigned char *bits, int y0, int y1)
{
    int n = 0, x, y;
    for (y = y0; y < y1; y++)
        for (x = 0; x < W; x++)
            if (bits[y * W + x]) n++;
    return n;
}

int main(void)
{
    struct { BITMAPINFOHEADER h; RGBQUAD pal[256]; } bi;
    unsigned char *bits = NULL, *guard;
    HDC dc;
    HBITMAP bm;
    int top, bottom, outside, i;
    FILE *f;
    memset(&bi, 0, sizeof bi);
    bi.h.biSize = sizeof bi.h;
    bi.h.biWidth = W;
    bi.h.biHeight = H;            /* positif : de bas en haut */
    bi.h.biPlanes = 1;
    bi.h.biBitCount = 8;
    for (i = 0; i < 256; i++) bi.pal[i].rgbRed = bi.pal[i].rgbGreen = bi.pal[i].rgbBlue = (BYTE)i;
    dc = CreateCompatibleDC(NULL);
    bm = CreateDIBSection(dc, (BITMAPINFO *)&bi, DIB_RGB_COLORS, (void **)&bits, NULL, 0);
    SelectObject(dc, bm);
    memset(bits, 0, W * H);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, RGB(255, 255, 255));
    TextOutA(dc, 1, 1, "AB", 2);
    /* lignes du haut de l'image = fin des bits */
    top = count(bits, H / 2, H);
    bottom = count(bits, 0, H / 2);
    memset(bits, 0, W * H);
    guard = bits;
    TextOutA(dc, -20, -20, "XYZ", 3);
    TextOutA(dc, W - 4, H - 4, "XYZ", 3);
    TextOutA(dc, 5000, 5000, "XYZ", 3);
    TextOutA(dc, -5000, 30, "XYZ", 3);
    outside = count(guard, 0, H);
    f = fopen("TEXTOUT.TXT", "w");
    fprintf(f, "top=%d bottom=%d outside=%d\n", top > 0, bottom, outside > 0);
    fclose(f);
    return 0;
}
