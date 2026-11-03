/* Contournement CIFS : cargo hardlinke les build-scripts puis les execve.
 * Sur certains montages CIFS, execve du hardlink d'un ELF frais renvoie EINVAL.
 * Ce preload fait echouer link/linkat (EXDEV) pour forcer une copie. */
#define _GNU_SOURCE
#include <errno.h>
#include <unistd.h>

int link(const char *oldpath, const char *newpath) {
    (void)oldpath;
    (void)newpath;
    errno = EXDEV;
    return -1;
}

int linkat(int olddirfd, const char *oldpath, int newdirfd, const char *newpath,
           int flags) {
    (void)olddirfd;
    (void)oldpath;
    (void)newdirfd;
    (void)newpath;
    (void)flags;
    errno = EXDEV;
    return -1;
}
