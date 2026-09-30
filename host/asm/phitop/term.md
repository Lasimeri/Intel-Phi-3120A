# term.S: the terminal

`term_open`: `TCGETS` on standard input (0 when it is not a terminal),
the copy with `ICANON` and `ECHO` cleared and `VMIN`/`VTIME` zero set
with `TCSETS` (the kernel's 36-byte `struct termios`,
`asm-generic/termbits.h`), SIGINT and SIGTERM handlers that set
`stop_flag` (installed with `rt_sigaction` and a restorer, which x86-64
requires), then `\e[?1049h\e[?25l\e[H\e[2J`: the alternate screen, the
cursor hidden, cleared. `term_close` reverses it. `term_size` is
`TIOCGWINSZ` on standard output, 80 by 24 when unknown. `term_key(ms)`
polls standard input and returns the first byte, 0 for an escape
sequence, -1 when nothing came.
