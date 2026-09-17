# phi-ring / ring.rs

One direction of one channel: a byte ring with one writer and one reader.

## Invariants

- `size` is a power of two, so `index & (size-1)` is the data offset and
  the ring can be filled completely (a "one slot empty" scheme is not needed
  because fullness is `head - tail == size`, not equality of indices).
- Indices are free-running `u32` counters; all differences are computed
  with wrapping arithmetic, so a ring keeps working across the 4 GiB
  index wrap.
- The producer reads `tail` and writes `head`; the consumer reads `head`
  and writes `tail`. Neither writes the other's index, which is what makes
  a lock unnecessary. Indices are read and written with `read_u32` /
  `write_u32`, one 32-bit access each on the card backend.
- Publishing an index happens after `fence()`, so the other side never sees
  an index that points at bytes still in flight (data before `head`; reads
  finished before `tail`).
- Neither end caches an index; both are re-read from memory on every call.

## Behavior at the boundaries

Pushing more than the free space accepts a prefix and returns its length;
the caller retries later. Popping into a buffer larger than what is
available returns what there is. Both are the natural behavior for a
console and for a frame queue with a length prefix (the network layer
never splits a frame across two `push` calls that could interleave, because
there is one producer).

## Recovery from inconsistent indices

`head - tail > size` cannot happen in a consistent ring. It is seen when
the other side restarted (a card reboot puts its `head` back to zero while
the host's `tail` is still large) or when the header was overwritten. The
producer's `free` then saturates to zero (the ring reads as full, `indices`
shows the raw values). The consumer's `available` reports zero and `pop`
sets `tail = head`, dropping whatever was in between, so the ring resumes
from the producer's index instead of delivering a full ring of stale bytes
in a loop. The card's drivers do the same on their side (`knc_blk.c`,
`knc_net.c`). The host's block service does not use `pop`'s resync because a
dropped block request hangs the card: it re-reads the indices and logs
(`phictl/src/disk.rs`).

## Testing

Header round trip, wraparound of the data area at its last byte, `u32`
index wraparound, exact fullness and emptiness of `free` and `available`,
partial pushes, a tail ahead of the head, rejection of bad headers, and the
consumer's resynchronisation. No hardware.
