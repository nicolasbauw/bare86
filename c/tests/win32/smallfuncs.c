/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt_win), programme de tests/hx_smallfuncs.rs
   Fonctions que HX n'avait pas (fork de HX de Bare86) et que des démos importent par leur nom : elles sont IMPORTÉES
   statiquement ici (sans elles, HX refuse de charger le programme), puis appelées. Résultats dans SMALL.TXT ; un cadre
   rouge d'un pixel (FrameRect) est dessiné en (100,100)-(200,160). */
#include <windows.h>
#include <mmsystem.h>
#include <stdio.h>
#include <string.h>

static LRESULT CALLBACK wp(HWND h, UINT m, WPARAM w, LPARAM l) { return DefWindowProc(h, m, w, l); }

int main(void)
{
    WNDCLASS wc;
    HWND hwnd;
    HDC hdc;
    RECT rc, clip, frame;
    LOGPEN lp;
    HPEN pen;
    HBRUSH red;
    MMTIME mt;
    POINT pt;
    FILE *f;
    int bk_default, bk_set, scroll, joy, wave, tgst, kl;
    MSG msg;
    DWORD t0;
    GUID g1, g2;
    HMODULE ole, cc;
    BOOL (WINAPI *isequal)(const GUID *, const GUID *);
    DWORD (WINAPI *cbv)(void);
    BOOL (WINAPI *icce)(void *);
    BYTE dib[64];
    HMODULE wi, c2;
    int wininet_ok, imagelist_ok;
    int sbm, hatch, dibpat, rrgn, guid1, guid2, ps, cbvr, iccer;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = wp; wc.hInstance = GetModuleHandle(NULL); wc.lpszClassName = "sf";
    RegisterClass(&wc);
    hwnd = CreateWindow("sf", "sf", WS_POPUP | WS_VISIBLE, 0, 0, 800, 600, NULL, NULL, wc.hInstance, NULL);
    hdc = GetDC(hwnd);
    f = fopen("SMALL.TXT", "w");
    GetClipCursor(&rc);
    fprintf(f, "clip0=%d,%d,%d,%d\n", (int)rc.left, (int)rc.top, (int)rc.right, (int)rc.bottom);
    clip.left = 10; clip.top = 20; clip.right = 300; clip.bottom = 200;
    ClipCursor(&clip);
    GetClipCursor(&rc);
    fprintf(f, "clip1=%d,%d,%d,%d\n", (int)rc.left, (int)rc.top, (int)rc.right, (int)rc.bottom);
    ClipCursor(NULL);
    GetClipCursor(&rc);
    fprintf(f, "clip2=%d,%d,%d,%d\n", (int)rc.left, (int)rc.top, (int)rc.right, (int)rc.bottom);
    bk_default = GetBkMode(hdc);
    SetBkMode(hdc, TRANSPARENT);
    bk_set = GetBkMode(hdc);
    fprintf(f, "bkmode=%d,%d\n", bk_default, bk_set);
    memset(&lp, 0, sizeof lp);
    lp.lopnStyle = PS_SOLID; lp.lopnWidth.x = 1; lp.lopnColor = RGB(0, 255, 0);
    pen = CreatePenIndirect(&lp);
    fprintf(f, "pen=%d\n", pen != NULL);
    SetBrushOrgEx(hdc, 3, 4, &pt);
    GetBrushOrgEx(hdc, &pt);
    fprintf(f, "brushorg=%d,%d\n", (int)pt.x, (int)pt.y);
    scroll = GetScrollPos(hwnd, SB_VERT);
    HideCaret(hwnd); ShowCaret(hwnd);
    kl = ActivateKeyboardLayout(NULL, 0) != NULL;
    fprintf(f, "scroll=%d caret_kl=%d\n", scroll, kl);
    joy = joySetThreshold(0, 100);
    wave = waveOutSetPlaybackRate(NULL, 0x10000);
    memset(&mt, 0, sizeof mt);
    tgst = timeGetSystemTime(&mt, sizeof mt);
    fprintf(f, "joy=%d wave=%d tgst=%d type=%d\n", joy, wave, tgst, (int)mt.wType);
    /* fonctions ajoutées ensuite : GDI32, WINMM, OLE32 (par GetProcAddress), COMCTL32 */
    SetStretchBltMode(hdc, COLORONCOLOR);
    sbm = GetStretchBltMode(hdc);
    hatch = CreateHatchBrush(HS_CROSS, RGB(1, 2, 3)) != NULL;
    memset(dib, 0, sizeof dib);
    dibpat = CreateDIBPatternBrushPt(dib, DIB_RGB_COLORS) != NULL;
    rrgn = CreateRoundRectRgn(0, 0, 10, 10, 3, 3) != NULL;
    ps = PlaySoundA("SystemAsterisk", NULL, SND_ALIAS | SND_NODEFAULT);
    ole = LoadLibrary("OLE32.DLL");
    isequal = ole ? (BOOL (WINAPI *)(const GUID *, const GUID *))GetProcAddress(ole, "IsEqualGUID") : NULL;
    cbv = ole ? (DWORD (WINAPI *)(void))GetProcAddress(ole, "CoBuildVersion") : NULL;
    memset(&g1, 7, sizeof g1); g2 = g1;
    guid1 = isequal ? isequal(&g1, &g2) : -1;
    g2.Data4[7] ^= 1;
    guid2 = isequal ? isequal(&g1, &g2) : -1;
    cbvr = cbv ? (int)cbv() : -1;
    cc = LoadLibrary("COMCTL32.DLL");
    icce = cc ? (BOOL (WINAPI *)(void *))GetProcAddress(cc, "InitCommonControlsEx") : NULL;
    iccer = icce ? icce(NULL) : -1;
    wi = LoadLibrary("WININET.DLL");
    wininet_ok = wi && GetProcAddress(wi, "InternetGetConnectedState") && GetProcAddress(wi, "FtpOpenFileA") && GetProcAddress(wi, "InternetSetFilePointer");
    c2 = cc;
    imagelist_ok = c2 && GetProcAddress(c2, "ImageList_Create") && GetProcAddress(c2, "ImageList_Draw");
    fprintf(f, "more2=wininet:%d imagelist:%d\n", wininet_ok != 0, imagelist_ok != 0);
    {   /* PolyPolygon, RoundRect (GDI32 de trx-elec, intro 64k qui résout ses imports elle-même), loin du cadre testé */
        HMODULE g = GetModuleHandle("GDI32.DLL");
        BOOL (WINAPI *ppg)(HDC, const POINT *, const INT *, int) = (BOOL (WINAPI *)(HDC, const POINT *, const INT *, int))GetProcAddress(g, "PolyPolygon");
        BOOL (WINAPI *rr)(HDC, int, int, int, int, int, int) = (BOOL (WINAPI *)(HDC, int, int, int, int, int, int))GetProcAddress(g, "RoundRect");
        static POINT pts[7] = { {500, 400}, {540, 400}, {520, 440}, {600, 400}, {640, 400}, {640, 440}, {600, 440} };
        static INT counts[2] = { 3, 4 };
        fprintf(f, "poly=%d,%d\n", ppg ? ppg(hdc, pts, counts, 2) : -1, rr ? rr(hdc, 500, 460, 560, 500, 8, 8) : -1);
    }
    {   /* addlib (intro 64k) : CreateBitmapIndirect, bmHeight positif (taille du tampon de GetObject), et une police
           inconnue donne quand même une police (HX n'en a qu'une sous DOS) */
        static BYTE bits[64 * 8];
        BITMAP bm, got;
        HBITMAP hb;
        LOGFONT lf;
        HFONT hf;
        memset(&bm, 0, sizeof bm);
        bm.bmWidth = 64; bm.bmHeight = 64; bm.bmWidthBytes = 8; bm.bmPlanes = 1; bm.bmBitsPixel = 1; bm.bmBits = bits;
        hb = CreateBitmapIndirect(&bm);
        memset(&got, 0, sizeof got);
        if (hb) GetObject(hb, sizeof got, &got);
        memset(&lf, 0, sizeof lf);
        lf.lfHeight = -24;
        strcpy(lf.lfFaceName, "Arial Black");
        hf = CreateFontIndirect(&lf);
        fprintf(f, "gdi2=bmi:%d h:%d size:%d font:%d\n", hb != NULL, (int)got.bmHeight, (int)(got.bmWidthBytes * got.bmHeight), hf != NULL);
        if (hf) DeleteObject(hf);
        if (hb) DeleteObject(hb);
    }
    fprintf(f, "more=stretch:%d hatch:%d dibpat:%d rrgn:%d ps:%d guid:%d,%d cbv:%x icce:%d\n", sbm, hatch, dibpat, rrgn, ps, guid1, guid2, cbvr, iccer);
    fclose(f);
    red = CreateSolidBrush(RGB(255, 0, 0));
    frame.left = 100; frame.top = 100; frame.right = 200; frame.bottom = 160;
    t0 = GetTickCount();
    while (GetTickCount() - t0 < 120000) {
        while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) { TranslateMessage(&msg); DispatchMessage(&msg); }
        FrameRect(hdc, &frame, red);
        Sleep(50);
    }
    ReleaseDC(hwnd, hdc);
    return 0;
}
