# ncurses for the card

`ncurses.sh` cross-builds ncurses 6.5 with `knc-cc`: wide-character
(`libncursesw.a`, terminfo included), static, no programs, and the usual
terminal descriptions (xterm, xterm-256color, vt100, linux, screen, tmux,
dumb, ansi and a few more) compiled in as fallbacks by the host's `tic`,
so a card with no terminfo files still drives a terminal over SSH. The
host build tools come from the host compiler (`--with-build-cc`).

Consumers: htop (`Lasimeri/htop-phi`) and CPython's `curses` module for
glances (`Lasimeri/glances-phi`). Installed into the card sysroot
(`toolchain/build/sysroot/usr/lib`, headers flat in `usr/include`), audited.
