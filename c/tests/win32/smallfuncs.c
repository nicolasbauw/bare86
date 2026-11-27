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
