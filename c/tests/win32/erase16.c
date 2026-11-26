/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt_win), programme de tests/hx_erase16.rs
   Une fenêtre à fond rouge dont la procédure coupe le wParam de WM_ERASEBKGND (le contexte d'affichage) à 16 bits avant de
   le passer à DefWindowProc, comme le fait la démo « haetten » (code d'origine Win16/Win9x, où les handles tiennent sur 16
   bits ; ici un handle est une adresse de 32 bits). Le fond rouge doit être peint, le programme ne doit pas planter. */
#include <windows.h>
#include <string.h>

static LRESULT CALLBACK wp(HWND h, UINT m, WPARAM w, LPARAM l)
{
    if (m == WM_ERASEBKGND) w &= 0xFFFF;
    return DefWindowProc(h, m, w, l);
}

int main(void)
{
    WNDCLASS wc;
    HWND hwnd;
    MSG msg;
    DWORD t0;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = wp;
    wc.hInstance = GetModuleHandle(NULL);
    wc.lpszClassName = "er";
    wc.hbrBackground = CreateSolidBrush(RGB(255, 0, 0));
    RegisterClass(&wc);
    hwnd = CreateWindow("er", "er", WS_POPUP | WS_VISIBLE, 100, 100, 200, 100, NULL, NULL, wc.hInstance, NULL);
    ShowWindow(hwnd, SW_SHOW);
    UpdateWindow(hwnd);
    t0 = GetTickCount();
    while (GetTickCount() - t0 < 120000) {
        while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) { TranslateMessage(&msg); DispatchMessage(&msg); }
        Sleep(50);
    }
    return 0;
}
