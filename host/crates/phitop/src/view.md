# view.rs: the frame

`render` lays out one card's frame for a terminal of `cols` by `rows`;
`render_multi` lays out one block per card (`card_block`), each given an
equal share of the rows, and picks what fits that share: the header line
always, then the temperatures, memory and the full grid with a few
processes when there are nine or more lines, else the mean load per core
and a memory line. A card without a sample shows its error on the header
line. The single-card frame:

| line | content |
| --- | --- |
| 1 | uptime, load averages, tasks, clock and voltage, mean load over all hardware threads, active threads, the interval (or the last error in red) |
| 2 | the nine die sensors (`--` where unfused), the hottest now, the peak since boot, board sensors when the card reports any |
| 3 | memory used of total with a bar, page cache, swap on host RAM |
| 4 | PCIe rates to and from the card, split into DMA engine and aperture |
| 5 | block devices (read, write) and interfaces (rx, tx) as the card counts them |
| grid | one column per physical core (57), one row per hardware thread (4), a mean row; each cell an eighth-block glyph whose height is the load, coloured grey, green, yellow, orange, red |
| table | processes sorted by CPU share or resident size; kernel threads in brackets and dimmed |
| last | the keys |

The grid uses two columns per core when the terminal is at least 121
wide (core numbers every five), else one (numbers every ten). Lines are
cut to the terminal width by visible characters, escape sequences not
counted, and every line is followed by an erase to end of line so a
narrower frame leaves nothing behind; the whole frame is one write after
a cursor home, without clearing the screen, so it does not flicker.

`--batch` renders the same frame without colour or cursor control.
