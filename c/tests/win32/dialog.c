/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt), programme de tests/hx_dialog.rs
   Boîtes de dialogue modales sous HX (DUSER32 du fork) : DialogBoxIndirectParam envoie WM_INITDIALOG, puis IDOK,
   puis IDCANCEL, puis WM_CLOSE, jusqu'à EndDialog. Résultats dans DIALOG.TXT. */
#include <windows.h>
#include <stdio.h>

static int flags;

/* se ferme sur IDOK avec 42 */
static BOOL CALLBACK dp_ok(HWND h, UINT m, WPARAM w, LPARAM l)
{
    if (m == WM_INITDIALOG) { flags |= (l == 1234) ? 1 : 0x100; return TRUE; }
    if (m == WM_COMMAND && LOWORD(w) == IDOK) { flags |= 2; EndDialog(h, 42); return TRUE; }
    return FALSE;
}

/* ne se ferme que sur IDCANCEL, avec 7 */
static BOOL CALLBACK dp_cancel(HWND h, UINT m, WPARAM w, LPARAM l)
{
    if (m == WM_INITDIALOG) return TRUE;
    if (m == WM_COMMAND && LOWORD(w) == IDCANCEL) { EndDialog(h, 7); return TRUE; }
    return FALSE;
}

/* ne se ferme que sur WM_CLOSE, avec 9 */
static BOOL CALLBACK dp_close(HWND h, UINT m, WPARAM w, LPARAM l)
{
    if (m == WM_CLOSE) { EndDialog(h, 9); return TRUE; }
    return FALSE;
}

/* ne se ferme jamais : le résultat doit être 1 (IDOK) et le programme ne doit pas rester bloqué */
static BOOL CALLBACK dp_never(HWND h, UINT m, WPARAM w, LPARAM l)
{
    return FALSE;
}

static int nested_inner;

/* ouvre une boîte imbriquée à l'initialisation ; EndDialog(h, 5) ferme la sienne seulement */
static BOOL CALLBACK dp_outer(HWND h, UINT m, WPARAM w, LPARAM l)
{
    static DWORD tpl[8];
    if (m == WM_INITDIALOG) { nested_inner = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, h, dp_ok, 1234); return TRUE; }
    if (m == WM_COMMAND && LOWORD(w) == IDOK) { EndDialog(h, 5); return TRUE; }
    return FALSE;
}

int main(void)
{
    static DWORD tpl[8];
    int r1, r2, r3, r4, r5;
    FILE *f;
    r1 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_ok, 1234);
    r2 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_cancel, 0);
    r3 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_close, 0);
    r4 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_never, 0);
    r5 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_outer, 0);
    f = fopen("DIALOG.TXT", "w");
    fprintf(f, "ok=%d flags=%d cancel=%d close=%d never=%d outer=%d inner=%d\n", r1, flags, r2, r3, r4, r5, nested_inner);
    fclose(f);
    return 0;
}
