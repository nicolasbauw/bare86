/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt), programme de tests/hx_dialog.rs
   Boîtes de dialogue modales sous HX (DUSER32 du fork) : DialogBoxIndirectParam envoie WM_INITDIALOG, puis IDOK,
   puis IDCANCEL, puis WM_CLOSE, jusqu'à EndDialog ; la boîte est une vraie fenêtre (SendMessage, GetWindowRect). Avec un modèle
   qui a un bouton par défaut (BS_DEFPUSHBUTTON, comme « Start » ou « Run » dans la boîte de réglages d'une démo), WM_COMMAND
   porte d'abord l'identifiant de ce bouton ; les cases cochées par la procédure se relisent ; une boîte non modale
   (CreateDialogIndirectParam) reçoit le clic de son bouton par défaut par la file de messages. Résultats dans DIALOG.TXT. */
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

/* se comporte comme beaucoup de procédures de dialogue : envoie un message à sa propre boîte et lit sa fenêtre */
static int msg_ok;
static BOOL CALLBACK dp_msg(HWND h, UINT m, WPARAM w, LPARAM l)
{
    if (m == WM_INITDIALOG) {
        RECT r;
        SendMessage(h, WM_SETICON, ICON_BIG, 0);
        msg_ok = IsWindow(h) && GetWindowRect(h, &r);
        return TRUE;
    }
    if (m == WM_COMMAND && LOWORD(w) == IDOK) { EndDialog(h, 3); return TRUE; }
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

/* modèle en mémoire : police (DS_SETFONT), un bouton ordinaire (1009) puis le bouton par défaut (1008) */
static DWORD dbuf[64];

static WORD *item(WORD *p, DWORD style, WORD id, WORD ch)
{
    if ((DWORD)p & 2) p++;
    *(DWORD *)p = style; p += 2; *(DWORD *)p = 0; p += 2;
    *p++ = 0; *p++ = 0; *p++ = 40; *p++ = 12;
    *p++ = id; *p++ = 0xFFFF; *p++ = 0x80; *p++ = ch; *p++ = 0; *p++ = 0;
    return p;
}

static LPCDLGTEMPLATE deftpl(void)
{
    WORD *p = (WORD *)dbuf;
    *(DWORD *)p = DS_SETFONT | WS_POPUP; p += 2; *(DWORD *)p = 0; p += 2;
    *p++ = 2; *p++ = 0; *p++ = 0; *p++ = 100; *p++ = 50;
    *p++ = 0; *p++ = 0; *p++ = 'T'; *p++ = 0;   /* menu, classe, titre */
    *p++ = 8; *p++ = 'A'; *p++ = 0;              /* police */
    p = item(p, WS_CHILD | BS_PUSHBUTTON, 1009, 'Q');
    item(p, WS_CHILD | BS_DEFPUSHBUTTON, 1008, 'S');
    return (LPCDLGTEMPLATE)dbuf;
}

/* boîte de réglages : coche ses valeurs par défaut, les relit sur « Start » (1008) et se ferme avec 77 */
static int checks;
static BOOL CALLBACK dp_def(HWND h, UINT m, WPARAM w, LPARAM l)
{
    if (m == WM_INITDIALOG) {
        CheckDlgButton(h, 1003, BST_CHECKED);
        CheckRadioButton(h, 1004, 1006, 1006);
        return TRUE;
    }
    if (m == WM_COMMAND && LOWORD(w) == 1008) {
        checks = IsDlgButtonChecked(h, 1004) * 100 + IsDlgButtonChecked(h, 1003) * 10 + (int)SendDlgItemMessage(h, 1006, BM_GETCHECK, 0, 0);
        EndDialog(h, 77);
        return TRUE;
    }
    if (m == WM_COMMAND && LOWORD(w) == IDOK) { EndDialog(h, 1); return TRUE; }
    return FALSE;
}

/* boîte non modale : « Start » posté dans la file, la boucle de messages s'arrête */
static int modeless_hit;
static BOOL CALLBACK dp_modeless(HWND h, UINT m, WPARAM w, LPARAM l)
{
    if (m == WM_COMMAND && LOWORD(w) == 1008) { modeless_hit = 1; DestroyWindow(h); PostQuitMessage(0); return TRUE; }
    return FALSE;
}

int main(void)
{
    static DWORD tpl[8];
    int r1, r2, r3, r4, r5, r6, r7;
    HWND hm;
    MSG msg;
    FILE *f;
    r1 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_ok, 1234);
    r2 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_cancel, 0);
    r3 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_close, 0);
    r4 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_never, 0);
    r5 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_outer, 0);
    r6 = DialogBoxIndirectParam(NULL, (LPCDLGTEMPLATE)tpl, NULL, dp_msg, 0);
    r7 = DialogBoxIndirectParam(NULL, deftpl(), NULL, dp_def, 0);
    hm = CreateDialogIndirectParam(NULL, deftpl(), NULL, dp_modeless, 0);
    if (hm)
        while (GetMessage(&msg, NULL, 0, 0) > 0) {
            if (!IsDialogMessage(hm, &msg)) { TranslateMessage(&msg); DispatchMessage(&msg); }
        }
    f = fopen("DIALOG.TXT", "w");
    fprintf(f, "ok=%d flags=%d cancel=%d close=%d never=%d outer=%d inner=%d msg=%d/%d def=%d checks=%03d modeless=%d\n", r1, flags, r2, r3, r4, r5, nested_inner, r6, msg_ok, r7, checks, modeless_hit);
    fclose(f);
    return 0;
}
