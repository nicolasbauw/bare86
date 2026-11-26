/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt_win), programme de tests/hx_idle.rs
   Une boîte de message qui attend : la boucle d'attente de HX ne doit pas occuper un cœur (INT 2Fh AX=1680h, « libérer la
   tranche de temps », que Bare86 endort). */
#include <windows.h>
int main(void)
{
    MessageBox(NULL, "waiting", "msgbox", MB_OK);
    return 0;
}
