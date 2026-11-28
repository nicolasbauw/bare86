/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt), programme de tests/hx_stdctl.rs
   DUSER32 du fork de HX de Bare86 : classes de contrôles standard (BUTTON, COMBOBOX...) créées par CreateWindowEx dans une
   fenêtre ordinaire (fenêtre de réglages faite à la main) ; un bouton BS_DEFPUSHBUTTON « clique » de lui-même (WM_COMMAND,
   BN_CLICKED, posté à son parent), un bouton ordinaire non ; EnumDisplaySettings donne une fréquence (60 Hz) et aucun mode
   à 15 bits (les modes 5-5-5 sont en 16 bits, comme sous Windows). Résultats dans STDCTL.TXT. */
#include <windows.h>
#include <stdio.h>

static int clicked_def, clicked_other, from_button;
static HWND hdef;

static LRESULT CALLBACK wp(HWND h, UINT m, WPARAM w, LPARAM l)
{
    if (m == WM_COMMAND && HIWORD(w) == BN_CLICKED) {
        if (LOWORD(w) == 11) { clicked_def++; from_button = (HWND)l == hdef; }
        if (LOWORD(w) == 10) clicked_other++;
    }
    return DefWindowProc(h, m, w, l);
}

int main(void)
{
    WNDCLASS wc;
    HWND main_w, combo;
    MSG msg;
    DEVMODE dm;
    int i, n60 = 0, n15 = 0, count, sel;
    FILE *f;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = wp;
    wc.hInstance = GetModuleHandle(NULL);
    wc.lpszClassName = "setup";
    RegisterClass(&wc);
    main_w = CreateWindow("setup", "Setup", WS_POPUP, 0, 0, 200, 100, NULL, NULL, wc.hInstance, NULL);
    CreateWindow("button", "Quit", WS_CHILD | BS_PUSHBUTTON, 0, 0, 50, 20, main_w, (HMENU)10, wc.hInstance, NULL);
    hdef = CreateWindow("BUTTON", "Start", WS_CHILD | BS_DEFPUSHBUTTON, 60, 0, 50, 20, main_w, (HMENU)11, wc.hInstance, NULL);
    combo = CreateWindow("ComboBox", "", WS_CHILD | CBS_DROPDOWNLIST, 0, 30, 100, 80, main_w, (HMENU)12, wc.hInstance, NULL);
    SendMessage(combo, CB_ADDSTRING, 0, (LPARAM)"640x480");
    SendMessage(combo, CB_ADDSTRING, 0, (LPARAM)"800x600");
    SendMessage(combo, CB_SETCURSEL, 1, 0);
    count = (int)SendMessage(combo, CB_GETCOUNT, 0, 0);
    sel = (int)SendMessage(combo, CB_GETCURSEL, 0, 0);
    while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) DispatchMessage(&msg);
    for (i = 0; ; i++) {
        memset(&dm, 0, sizeof dm);
        dm.dmSize = sizeof dm;
        if (!EnumDisplaySettings(NULL, i, &dm)) break;
        if ((dm.dmFields & DM_DISPLAYFREQUENCY) && dm.dmDisplayFrequency == 60) n60++;
        if (dm.dmBitsPerPel == 15) n15++;
    }
    f = fopen("STDCTL.TXT", "w");
    fprintf(f, "created=%d def=%d/%d other=%d combo=%d/%d modes=%d hz60=%d bpp15=%d\n", hdef != NULL && combo != NULL,
            clicked_def, from_button, clicked_other, count, sel, i > 10, n60 == i, n15);
    fclose(f);
    return 0;
}
