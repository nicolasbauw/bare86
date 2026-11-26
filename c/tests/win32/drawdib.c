/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt_win), programme de tests/hx_drawdib.rs
   DrawDibDraw de MSVFW32.DLL (fork de HX de Bare86), chargée par LoadLibrary comme le font les démos.
   Image de 2×2 pixels en 32 bits, de bas en haut : ligne du bas bleu, blanc ; ligne du haut rouge, vert.
   1) en (100,100), 80×80, toute l'image ; 2) en (300,100), le seul pixel en haut à gauche (xSrc=0, ySrc=0,
   dxSrc=dySrc=1 : l'origine du rectangle source est son coin supérieur gauche) ; 3) en (100,300), l'image
   entière avec lpBits = NULL (les pixels suivent l'en-tête). */
#include <windows.h>
#include <string.h>

typedef LPVOID (WINAPI *PFNOPEN)(void);
typedef BOOL (WINAPI *PFNDRAW)(LPVOID, HDC, int, int, int, int, LPBITMAPINFOHEADER, LPVOID, int, int, int, int, UINT);

static LRESULT CALLBACK wp(HWND h, UINT m, WPARAM w, LPARAM l) { return DefWindowProc(h, m, w, l); }

int main(void)
{
    static struct { BITMAPINFOHEADER h; DWORD px[4]; } dib;
    WNDCLASS wc;
    HWND hwnd;
    HDC hdc;
    MSG msg;
    DWORD t0;
    HMODULE m;
    PFNOPEN open;
    PFNDRAW draw;
    LPVOID hdd;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = wp; wc.hInstance = GetModuleHandle(NULL); wc.lpszClassName = "dd";
    RegisterClass(&wc);
    hwnd = CreateWindow("dd", "dd", WS_POPUP | WS_VISIBLE, 0, 0, 800, 600, NULL, NULL, wc.hInstance, NULL);
    dib.h.biSize = sizeof(BITMAPINFOHEADER);
    dib.h.biWidth = 2; dib.h.biHeight = 2; dib.h.biPlanes = 1; dib.h.biBitCount = 32; dib.h.biCompression = BI_RGB;
    dib.px[0] = 0x000000FF; dib.px[1] = 0x00FFFFFF;     /* ligne du bas : bleu, blanc */
    dib.px[2] = 0x00FF0000; dib.px[3] = 0x0000FF00;     /* ligne du haut : rouge, vert */
    m = LoadLibrary("MSVFW32.DLL");
    if (!m) return 1;
    open = (PFNOPEN)GetProcAddress(m, "DrawDibOpen");
    draw = (PFNDRAW)GetProcAddress(m, "DrawDibDraw");
    if (!open || !draw) return 2;
    hdd = open();
    if (!hdd) return 3;
    t0 = GetTickCount();
    while (GetTickCount() - t0 < 120000) {
        while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) { TranslateMessage(&msg); DispatchMessage(&msg); }
        hdc = GetDC(hwnd);
        draw(hdd, hdc, 100, 100, 80, 80, &dib.h, dib.px, 0, 0, -1, -1, 0);
        draw(hdd, hdc, 300, 100, 80, 80, &dib.h, dib.px, 0, 0, 1, 1, 0);
        draw(hdd, hdc, 100, 300, 80, 80, &dib.h, NULL, 0, 0, -1, -1, 0);
        ReleaseDC(hwnd, hdc);
        Sleep(50);
    }
    return 0;
}
