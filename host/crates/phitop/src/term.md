# term.rs: the terminal

Only what a full-screen viewer needs, straight from libc (no TUI crate):

- `Term::open` turns off canonical mode and echo on standard input, keeps
  output post-processing (so `\n` still moves to the next line), switches
  to the alternate screen and hides the cursor. `Drop` restores all of
  it, so `q`, Ctrl-C and SIGTERM all leave the shell as it was: the
  signal handler only raises `STOP`, the main loop returns, and the
  `Term` drops on the way out.
- `Term::size` asks with TIOCGWINSZ each frame, so a resized window is
  picked up at the next redraw without handling SIGWINCH.
- `Term::key` polls standard input for at most the given time and reads
  what is there; an escape sequence is reported as key 0 and ignored.

When standard input is not a terminal (`--batch`, or a pipe) none of
this runs and frames go to standard output as plain text.
