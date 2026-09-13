# phi-ring / ring.rs

One direction of one channel: a byte ring with one writer and one reader.

## Invariants

- `size` is a power of two, so `index & (size-1)` is the data offset and
  the ring can be filled completely (a "one slot empty" scheme is not needed
  because fullness is `head - tail == size`, not equality of indices).
- The producer reads `tail` and writes `head`; the consumer reads `head`
  and writes `tail`. Neither writes the other's index, which is what makes
  a lock unnecessary.
- Publishing an index happens after `fence()`, so the other side never sees
  an index that points at bytes still in flight.

## Behavior at the boundaries

Pushing more than the free space accepts a prefix and returns its length;
the caller retries later. Popping into a buffer larger than what is
available returns what there is. Both are the natural behavior for a
console and for a frame queue with a length prefix (the network layer
never splits a frame across two `push` calls that could interleave, because
there is one producer).

## Testing

Wraparound of the data area, `u32` index wraparound, exact fullness, and
rejection of unformatted memory. No hardware.
