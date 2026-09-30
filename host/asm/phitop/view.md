# view.S: rendering

One frame as text, `rows` lines of at most `cols` visible characters,
as the Rust `view.rs` drew it. Each line is built in `line_buf` through
the write cursor and fitted into `frame_buf`: escape sequences are
copied without counting, a multi-byte UTF-8 character counts once, a
colour left open is closed with `\e[0m`; in colour every line ends with
`\e[K` and the frame starts with `\e[H`, so a redraw overwrites in
place without a clear; the last row gets no newline in colour.

**The one-card frame**: the header (uptime as h:mm:ss, the three load
averages, running and total processes, threads and their mean busy
percentage, the sample interval or the error in red), the temperature
line (nine die sensors or `--`, the highest, the peak since boot, the
clock and voltage, the board sensors that read above zero or "board
sensors: n/a (SMC)"), memory and swap with bars, PCIe rates by path and
direction, the card's disk and network rates, the core grid (a ruler
numbering every fifth core, one row per hardware thread, a mean row;
cells two columns wide when the ruler fits, else one; eighth blocks for
the load, 256-colour grey to red), the process table (PID, state, CPU
share with one decimal, resident size, threads, name; kernel threads in
brackets and dimmed, shown only with `k`; sorted by share then size or
size then share, descending, by heapsort over an index array), padding,
and the footer.

**Several cards**: each card an equal share of the rows (`(rows - 2 -
(n - 1)) / n`, at least 1), blocks separated by a blank line; a block
of nine lines or more has the temperature line, memory, swap and PCIe,
and the grid; a smaller one the mean row and one line of memory; a
card without a sample says so on its header line (the error, or "no
sample yet"); processes fill what remains of the block; two footer
lines with the card count, the interval or the error, and the keys.

Units: rates as `{:.2} GB/s`, `{:.1} MB/s`, `{:.1} kB/s` or `N B/s`;
sizes as `{:.1} GiB` from 10 GiB, `N MiB` from 10 MiB, `N KiB`; bars
`[###...]` with the filled part rounded. Decimals are rounded half up
(the Rust formatting rounded to nearest even on the exact binary value,
which differs only on exact ties).
