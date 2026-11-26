/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt_win), programme de tests/hx_stretch.rs
   Test de StretchDIBits : image 4x4 en 32 bits, bas en haut (hauteur positive).
   Coins : haut-gauche rouge, haut-droite vert, bas-gauche bleu, bas-droite blanc ; le reste noir.
   Etiree à 160x160 en (100,100) : 1) tout l'écran-test, 2) une partie (2x2 centrale) en (400,100). */
#include <windows.h>
#include <string.h>
static LRESULT CALLBACK wp(HWND h, UINT m, WPARAM w, LPARAM l)
{
    if (m == WM_DESTROY) PostQuitMessage(0);
    return DefWindowProc(h, m, w, l);
}

int main(void)
{
    static DWORD bits[16];
    BITMAPINFO bi;
    WNDCLASS wc;
    HWND hwnd;
    HDC hdc;
    MSG msg;
    DWORD t0;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = wp;
    wc.hInstance = GetModuleHandle(NULL);
    wc.lpszClassName = "st";
    wc.hbrBackground = (HBRUSH)GetStockObject(BLACK_BRUSH);
    RegisterClass(&wc);
    hwnd = CreateWindow("st", "st", WS_POPUP | WS_VISIBLE, 0, 0, 800, 600, NULL, NULL, wc.hInstance, NULL);
    memset(&bi, 0, sizeof bi);
    bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth = 4;
    bi.bmiHeader.biHeight = 4;          /* positif : bas en haut, 1re ligne en memoire = en bas */
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    memset(bits, 0, sizeof bits);
    bits[0 * 4 + 0] = 0x000000FF;       /* bas-gauche : bleu  (0x00RRGGBB) */
    bits[0 * 4 + 3] = 0x00FFFFFF;       /* bas-droite : blanc */
    bits[3 * 4 + 0] = 0x00FF0000;       /* haut-gauche : rouge */
    bits[3 * 4 + 3] = 0x0000FF00;       /* haut-droite : vert */
    t0 = GetTickCount();
    while (GetTickCount() - t0 < 120000) {
        while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) { TranslateMessage(&msg); DispatchMessage(&msg); }
        hdc = GetDC(hwnd);
        StretchDIBits(hdc, 100, 100, 160, 160, 0, 0, 4, 4, bits, &bi, DIB_RGB_COLORS, SRCCOPY);
        StretchDIBits(hdc, 400, 100, 160, 160, 0, 0, 2, 2, bits, &bi, DIB_RGB_COLORS, SRCCOPY);   /* 2x2 du coin bas-gauche */
        ReleaseDC(hwnd, hdc);
        Sleep(50);
    }
    return 0;
}
