/* SPDX-License-Identifier: GPL-3.0-or-later */
/* Étape 3 : DOS en HLE vu par le runtime C d'Open Watcom (modèle small,
 * exécutable MZ). Chaque contrôle affiche « ok » ou « ÉCHEC » ; le code de
 * sortie est le nombre d'échecs. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <direct.h>
#include <dos.h>
#include <io.h>
#include <fcntl.h>
#include <process.h>

static int failures;

static void check(const char *what, int ok)
{
    printf("%s : %s\n", what, ok ? "ok" : "ECHEC");
    if (!ok)
        failures++;
}

int main(int argc, char **argv)
{
    char buf[64];
    char cwd[80];
    FILE *f;
    struct find_t ff;
    int n, found, code;
    void *big;

    check("argv", argc == 3 && strcmp(argv[1], "un") == 0 && strcmp(argv[2], "deux") == 0);
    check("argv[0]", strstr(argv[0], "FILES.EXE") != NULL);
    check("getenv", getenv("COMSPEC") != NULL);

    f = fopen("ESSAI.TXT", "w");
    check("fopen w", f != NULL);
    fprintf(f, "ligne 1\nligne 2\n");
    fclose(f);
    f = fopen("ESSAI.TXT", "r");
    check("fopen r", f != NULL);
    fgets(buf, sizeof buf, f);
    check("fgets", strcmp(buf, "ligne 1\n") == 0);
    fseek(f, 0, SEEK_END);
    check("taille", ftell(f) == 18);
    fclose(f);

    found = 0;
    if (_dos_findfirst("*.TXT", _A_NORMAL, &ff) == 0) {
        do {
            if (strcmp(ff.name, "ESSAI.TXT") == 0 && ff.size == 18)
                found = 1;
        } while (_dos_findnext(&ff) == 0);
    }
    check("findfirst", found);

    check("mkdir", mkdir("SOUSREP") == 0);
    check("chdir", chdir("SOUSREP") == 0);
    check("getcwd", getcwd(cwd, sizeof cwd) != NULL && strcmp(cwd, "C:\\SOUSREP") == 0);
    check("rename", rename("..\\ESSAI.TXT", "DEPLACE.TXT") == 0);
    check("open ..", open("..\\ESSAI.TXT", O_RDONLY) == -1);
    check("remove", remove("DEPLACE.TXT") == 0);
    chdir("..");
    check("rmdir", rmdir("SOUSREP") == 0);

    big = malloc(30000);
    check("malloc", big != NULL);
    free(big);

    code = spawnl(P_WAIT, "CHILD.EXE", "CHILD.EXE", "salut", NULL);
    check("spawn", code == 7);

    n = printf("fin\n");
    return failures;
}
